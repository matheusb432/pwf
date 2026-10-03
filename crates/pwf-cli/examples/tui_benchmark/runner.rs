use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail, ensure};
use clap::Parser;
use expectrl::{Expect as _, session::OsSession};
use serde::{Deserialize, Serialize};

const SAMPLES: usize = 5;
const WIDTH: u16 = 140;
const HEIGHT: u16 = 40;
const IDLE: Duration = Duration::from_secs(3);
const NAVIGATION_KEYS: usize = 240;
const SEARCHES: usize = 32;
const FIXTURE_SCHEMA: u32 = 1;
const WORKLOADS: [(&str, usize, usize); 4] = [
    ("small", 200, 2048),
    ("medium", 2000, 4096),
    ("large", 4000, 3072),
    // Keep the long-body page below the 4 MiB RPC limit.
    ("long-bodies", 24, 128 * 1024),
];

#[derive(Parser)]
struct Arguments {
    #[arg(long)]
    update: bool,
    #[arg(long)]
    binary: Option<PathBuf>,
    #[arg(long)]
    server: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Configuration {
    schema: u32,
    fixture_schema: u32,
    profile: String,
    pty: String,
    os: String,
    architecture: String,
    cpu: String,
    clock_ticks: u32,
    samples: usize,
    width: u16,
    height: u16,
    idle_ms: u64,
    navigation_keys: usize,
    searches: usize,
    search_queries: Vec<String>,
    server_hash: String,
}

#[derive(Deserialize, Serialize)]
struct Report {
    configuration: Configuration,
    binary_hash: String,
    cases: Vec<Case>,
}

#[derive(Deserialize, Serialize)]
struct Case {
    name: String,
    records: usize,
    body_bytes: usize,
    samples: Vec<Sample>,
}

#[derive(Deserialize, Serialize)]
struct Sample {
    startup_ms: f64,
    startup_cpu_ms: f64,
    idle_cpu_ms: f64,
    navigation_cpu_ms: f64,
    search_cpu_ms: f64,
    cpu_ms: f64,
    rss_loaded_kib: u32,
    rss_final_kib: u32,
    rss_peak_kib: u32,
}

struct Fixture {
    root: tempfile::TempDir,
    server: Child,
}

impl Fixture {
    fn new(binary: &Path, server: &Path, records: usize, body_bytes: usize) -> Result<Self> {
        let root = tempfile::tempdir()?;
        for directory in ["tasks", "source", "config", "state", "data"] {
            fs::create_dir(root.path().join(directory))?;
        }
        for number in 1..=records {
            let line = "- Verify saved Markdown, keyboard navigation, and draft recovery.\n";
            let marker = number % 8;
            let body = format!(
                "## Goals\n\n{}\nSaved needle{marker} / conteúdo útil / ΑΒΓ\n",
                line.repeat(body_bytes / line.len())
            );
            fs::write(
                root.path().join(format!("tasks/BEN-{number:04}.md")),
                format!(
                    "---\nid: BEN-{number:04}\ntitle: Benchmark record {number}\nstatus: active\ncreated_at: 2026-09-13T00:00:00Z\n---\n\n{body}"
                ),
            )?;
        }
        let log = fs::File::create(root.path().join("server.log"))?;
        let mut command = Command::new(server);
        configure(&mut command, root.path());
        let child = command
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()?;
        let mut fixture = Self {
            root,
            server: child,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut probe = fixture.command(binary);
            if probe.args(["project", "list"]).output()?.status.success() {
                break;
            }
            ensure!(
                fixture.server.try_wait()?.is_none(),
                "Benchmark server exited: {}",
                fs::read_to_string(fixture.root.path().join("server.log"))?
            );
            ensure!(
                Instant::now() < deadline,
                "Benchmark server readiness timed out."
            );
            thread::sleep(Duration::from_millis(20));
        }
        let project = serde_json::json!({
            "id": "BEN", "title": "TUI benchmark",
            "source": {"value": fixture.root.path().join("source")},
            "tasks": {"kind": "directory", "path": fixture.root.path().join("tasks")},
        });
        let output = fixture
            .command(binary)
            .args([
                "project",
                "add",
                "--kind",
                "directory",
                &project.to_string(),
            ])
            .output()?;
        ensure!(
            output.status.success(),
            "Cannot register benchmark project: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = fixture
            .command(binary)
            .args(["task", "list", "--project", "BEN", "--all"])
            .output()?;
        ensure!(
            output.status.success(),
            "Cannot prime benchmark records: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(fixture)
    }

    fn command(&self, binary: &Path) -> Command {
        let mut command = Command::new(binary);
        configure(&mut command, self.root.path());
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

fn configure(command: &mut Command, root: &Path) {
    command
        .current_dir(root)
        .env("PWF_DATABASE_PATH", root.join("projects.sqlite3"))
        .env("PWF_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("TERM", "xterm-256color")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("FORCE_COLOR");
}

struct Terminal {
    session: OsSession,
    screen: vt100::Parser,
    pid: i32,
}

impl Terminal {
    fn spawn(mut command: Command) -> Result<Self> {
        command.args(["tui", "BEN"]);
        let mut session = expectrl::Session::spawn(command)?;
        session.set_expect_timeout(Some(Duration::from_secs(35)));
        session.get_process_mut().set_window_size(WIDTH, HEIGHT)?;
        let pid = session.get_process().pid().as_raw();
        Ok(Self {
            session,
            screen: vt100::Parser::new(HEIGHT, WIDTH, 0),
            pid,
        })
    }

    fn drain(&mut self) -> Result<()> {
        let mut bytes = [0; 8192];
        for _ in 0..256 {
            match self.session.try_read(&mut bytes) {
                Ok(0) => bail!(
                    "Benchmark terminal closed: {}",
                    self.screen.screen().contents()
                ),
                Ok(count) => self.screen.process(&bytes[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn expect(&mut self, text: &str) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(35);
        loop {
            self.drain()?;
            if self.screen.screen().contents().contains(text) {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "Missing terminal text {text:?}: {}",
                self.screen.screen().contents()
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn send(&mut self, text: &str) -> Result<()> {
        self.session.send(text)?;
        Ok(())
    }

    fn idle(&mut self, duration: Duration) -> Result<()> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.drain()?;
            thread::sleep(Duration::from_millis(4));
        }
        Ok(())
    }
}

struct Resources {
    ticks: u32,
    rss_kib: u32,
    peak_kib: u32,
}

fn resources(pid: i32) -> Result<Resources> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let fields = stat
        .rsplit_once(')')
        .context("Invalid /proc process stat")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
    let value = |name: &str| -> Result<u32> {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .context("Missing /proc memory counter")?
            .split_whitespace()
            .next()
            .context("Missing /proc memory value")?
            .parse()
            .map_err(Into::into)
    };
    Ok(Resources {
        ticks: fields.get(11).context("Missing utime")?.parse::<u32>()?
            + fields.get(12).context("Missing stime")?.parse::<u32>()?,
        rss_kib: value("VmRSS:")?,
        peak_kib: value("VmHWM:")?,
    })
}

fn measure(fixture: &Fixture, binary: &Path, records: usize, clock_ticks: u32) -> Result<Sample> {
    let started = Instant::now();
    let mut terminal = Terminal::spawn(fixture.command(binary))?;
    terminal.expect(&format!("Loaded {records} saved records"))?;
    let startup_ms = started.elapsed().as_secs_f64() * 1000.0;
    let loaded = resources(terminal.pid)?;
    terminal.idle(IDLE)?;
    let idle = resources(terminal.pid)?;
    for index in 0..NAVIGATION_KEYS {
        terminal.send(if index % 40 < 20 { "j" } else { "k" })?;
        terminal.idle(Duration::from_millis(4))?;
    }
    terminal.send("\x1b[H")?;
    terminal.expect(&format!("BEN-{records:04}"))?;
    terminal.idle(Duration::from_millis(150))?;
    let navigation = resources(terminal.pid)?;
    terminal.send("f/")?;
    terminal.expect("saved contents")?;
    for index in 0..SEARCHES {
        let query = search_query(index);
        terminal.send(&format!("\x01\x0b\x1b[200~{query}\x1b[201~"))?;
        let count = if query.starts_with("needle") {
            records / 8
        } else {
            records
        };
        terminal.expect(&format!("{count} visible / {records} loaded"))?;
        let marker = if query.starts_with("needle") {
            format!("Saved {query}")
        } else {
            query.to_string()
        };
        terminal.expect(&marker)?;
    }
    terminal.send("\r")?;
    terminal.idle(Duration::from_millis(150))?;
    let final_resources = resources(terminal.pid)?;
    terminal.send("q")?;
    terminal.session.expect(expectrl::Eof)?;
    ensure!(
        matches!(
            terminal.session.get_process().wait()?,
            expectrl::process::unix::WaitStatus::Exited(_, 0)
        ),
        "TUI did not exit successfully."
    );
    let cpu_ms = |ticks| f64::from(ticks) * 1000.0 / f64::from(clock_ticks);
    Ok(Sample {
        startup_ms,
        startup_cpu_ms: cpu_ms(loaded.ticks),
        idle_cpu_ms: cpu_ms(idle.ticks - loaded.ticks),
        navigation_cpu_ms: cpu_ms(navigation.ticks - idle.ticks),
        search_cpu_ms: cpu_ms(final_resources.ticks - navigation.ticks),
        cpu_ms: cpu_ms(final_resources.ticks),
        rss_loaded_kib: loaded.rss_kib,
        rss_final_kib: final_resources.rss_kib,
        rss_peak_kib: final_resources.peak_kib,
    })
}

fn search_query(index: usize) -> &'static str {
    [
        "needle0",
        "needle1",
        "needle2",
        "needle3",
        "needle4",
        "needle5",
        "needle6",
        "needle7",
        "conteúdo",
        "αβγ",
    ][index % 10]
}

fn command_text(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    ensure!(
        output.status.success(),
        "Benchmark metadata command failed."
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn write_report(path: &Path, report: &Report) -> Result<()> {
    let mut staged =
        tempfile::NamedTempFile::new_in(path.parent().context("Report path has no parent")?)?;
    serde_json::to_writer_pretty(staged.as_file_mut(), report)?;
    staged.persist(path)?;
    Ok(())
}

fn freeze_server(server: &Path, path: &Path) -> Result<()> {
    let staged =
        tempfile::NamedTempFile::new_in(path.parent().context("Server path has no parent")?)?;
    fs::copy(server, staged.path())?;
    staged.persist(path)?;
    Ok(())
}

fn configuration(server: &Path) -> Result<Configuration> {
    let cpu = fs::read_to_string("/proc/cpuinfo")?
        .lines()
        .find_map(|line| line.strip_prefix("model name\t: "))
        .context("Missing CPU model")?
        .to_string();
    let clock_ticks = command_text(Command::new("getconf").arg("CLK_TCK"))?.parse()?;
    Ok(Configuration {
        schema: 3,
        fixture_schema: FIXTURE_SCHEMA,
        profile: "release".into(),
        pty: "expectrl-0.9/close-range".into(),
        os: command_text(Command::new("uname").arg("-sr"))?,
        architecture: std::env::consts::ARCH.into(),
        cpu,
        clock_ticks,
        samples: SAMPLES,
        width: WIDTH,
        height: HEIGHT,
        idle_ms: IDLE.as_millis().try_into()?,
        navigation_keys: NAVIGATION_KEYS,
        searches: SEARCHES,
        search_queries: (0..SEARCHES)
            .map(|index| search_query(index).to_string())
            .collect(),
        server_hash: command_text(Command::new("sha256sum").arg(server))?
            .split_whitespace()
            .next()
            .context("Missing server hash")?
            .to_string(),
    })
}

fn read_baseline(
    path: &Path,
    configuration: &Configuration,
    update: bool,
) -> Result<Option<Report>> {
    if !path.exists() {
        ensure!(update, "TUI baseline is missing; capture it with --update.");
        return Ok(None);
    }
    let decoded = serde_json::from_slice::<Report>(&fs::read(path)?);
    let report = match decoded {
        Ok(report) => report,
        Err(error) if update => {
            eprintln!("Replacing an incompatible TUI baseline: {error}");
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let mut expected = configuration.clone();
    expected
        .server_hash
        .clone_from(&report.configuration.server_hash);
    let compatible = report.configuration == expected
        && report.cases.len() == WORKLOADS.len()
        && report
            .cases
            .iter()
            .zip(WORKLOADS)
            .all(|(case, (name, records, body_bytes))| {
                case.name == name
                    && case.records == records
                    && case.body_bytes == body_bytes
                    && case.samples.len() == SAMPLES
            });
    if compatible {
        if report.configuration.server_hash != configuration.server_hash {
            println!("The backend executable changed; its effect on frontend startup is included.");
        }
        Ok(Some(report))
    } else {
        ensure!(
            update,
            "TUI baseline is incompatible; capture a fresh baseline with --update."
        );
        Ok(None)
    }
}

pub(super) fn run() -> Result<()> {
    let arguments = Arguments::parse();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = arguments
        .binary
        .unwrap_or_else(|| root.join("target/release/pwf"));
    let artifact = root.join(".artifacts/benchmarks/tui");
    fs::create_dir_all(&artifact)?;
    let server_frozen = artifact.join("pwf-server");
    let server = arguments
        .server
        .unwrap_or_else(|| root.join("target/release/pwf-server"));
    let configuration = configuration(&server)?;
    let clock_ticks = configuration.clock_ticks;
    let baseline_path = artifact.join("baseline.json");
    let baseline = read_baseline(&baseline_path, &configuration, arguments.update)?;
    let mut cases = Vec::new();
    for (name, records, body_bytes) in WORKLOADS {
        let fixture = Fixture::new(&binary, &server, records, body_bytes)?;
        let mut samples = Vec::new();
        for index in 0..SAMPLES {
            let sample = measure(&fixture, &binary, records, clock_ticks)?;
            println!(
                "{name} sample {}: CPU {:.0} ms, RSS {} KiB, peak {} KiB",
                index + 1,
                sample.cpu_ms,
                sample.rss_final_kib,
                sample.rss_peak_kib
            );
            samples.push(sample);
        }
        cases.push(Case {
            name: name.into(),
            records,
            body_bytes,
            samples,
        });
    }
    let report = Report {
        configuration,
        binary_hash: command_text(Command::new("sha256sum").arg(&binary))?
            .split_whitespace()
            .next()
            .context("Missing binary hash")?
            .to_string(),
        cases,
    };
    write_report(&artifact.join("current.json"), &report)?;
    if let Some(baseline) = baseline {
        for (before, after) in baseline.cases.iter().zip(&report.cases) {
            ensure!(
                before.name == after.name
                    && before.records == after.records
                    && before.body_bytes == after.body_bytes,
                "TUI workloads are incompatible."
            );
            let before_cpu = median(before.samples.iter().map(|sample| sample.cpu_ms).collect());
            let after_cpu = median(after.samples.iter().map(|sample| sample.cpu_ms).collect());
            let before_rss = median(
                before
                    .samples
                    .iter()
                    .map(|sample| f64::from(sample.rss_final_kib))
                    .collect(),
            );
            let after_rss = median(
                after
                    .samples
                    .iter()
                    .map(|sample| f64::from(sample.rss_final_kib))
                    .collect(),
            );
            println!(
                "{}: CPU {:.1}% reduction ({before_cpu:.0} → {after_cpu:.0} ms), RSS {:.1}% reduction ({before_rss:.0} → {after_rss:.0} KiB)",
                after.name,
                (1.0 - after_cpu / before_cpu) * 100.0,
                (1.0 - after_rss / before_rss) * 100.0
            );
        }
    }
    if arguments.update {
        freeze_server(&server, &server_frozen)?;
        write_report(&baseline_path, &report)?;
    }
    Ok(())
}
