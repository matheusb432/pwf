use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, ensure};
use pwf_client::{PwfClient, pb};
use pwf_local_transport::LocalEndpoint;
use serde::{Deserialize, Serialize};

const SAMPLES: usize = 7;
const RPC_REPETITIONS: u32 = 20;
const DETAILED_RPC_REPETITIONS: u32 = 3;
const CONCURRENT_READERS: usize = 4;
const WORKLOADS: [(&str, usize, usize); 3] = [
    ("small", 200, 512),
    ("medium", 1024, 2048),
    ("large", 2000, 3072),
];
const FORMATS: [&str; 4] = ["summary", "md", "json", "rich"];

#[derive(Default)]
struct Arguments {
    update: bool,
    binary: Option<PathBuf>,
    server: Option<PathBuf>,
}

impl Arguments {
    fn parse() -> Result<Self> {
        let mut arguments = Self::default();
        let mut values = std::env::args_os().skip(1);
        while let Some(value) = values.next() {
            match value.to_str() {
                Some("--update") => arguments.update = true,
                Some("--binary") => {
                    arguments.binary =
                        Some(values.next().context("--binary requires a path")?.into());
                }
                Some("--server") => {
                    arguments.server =
                        Some(values.next().context("--server requires a path")?.into());
                }
                _ => anyhow::bail!("Expected --update, --binary PATH, or --server PATH"),
            }
        }
        Ok(arguments)
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Configuration {
    schema: u32,
    profile: String,
    os: String,
    cpu: String,
    cpus: usize,
    rustc: String,
    samples: usize,
    rpc_repetitions: u32,
    detailed_rpc_repetitions: u32,
    concurrent_readers: usize,
    logging: String,
    cli_runner: String,
    workloads: Vec<(String, usize, usize)>,
    formats: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Report {
    configuration: Configuration,
    binary_hash: String,
    server_hash: String,
    #[serde(default)]
    load_average_start: Option<[f64; 3]>,
    #[serde(default)]
    load_average_end: Option<[f64; 3]>,
    cases: Vec<Case>,
}

#[derive(Deserialize, Serialize)]
struct Case {
    name: String,
    samples: Vec<Sample>,
}

#[derive(Deserialize, Serialize)]
struct Sample {
    latency_ms: f64,
    rss_peak_kib: u32,
    rss_resident_kib: u32,
}

struct Fixture {
    root: tempfile::TempDir,
    server: Child,
    client: PwfClient,
    sources: Vec<String>,
}

impl Fixture {
    async fn new(binary: &Path, server: &Path, records: usize, body_bytes: usize) -> Result<Self> {
        let root = tempfile::tempdir()?;
        for directory in ["tasks", "source", "config", "state", "data"] {
            fs::create_dir(root.path().join(directory))?;
        }
        let mut sources = Vec::with_capacity(records);
        for number in 1..=records {
            let line = "- Preserve authored Markdown, metadata, and pagination.\n";
            let source = format!(
                "---\nid: BEN-{number:04}\ntitle: Benchmark record {number}\nstatus: active\ncreated_at: 2026-09-13T00:00:00Z\n---\n\n## Goals\n\n{}",
                line.repeat(body_bytes / line.len())
            );
            fs::write(
                root.path().join(format!("tasks/BEN-{number:04}.md")),
                &source,
            )?;
            sources.push(source);
        }
        let mut command = Command::new(server);
        configure(&mut command, root.path());
        let child = command
            .env("RUST_LOG", "info")
            .stdout(Stdio::null())
            .stderr(fs::File::create(root.path().join("server.log"))?)
            .spawn()?;
        // Install cleanup before readiness or registration can fail.
        let mut child = ServerStarting(Some(child));
        let endpoint = LocalEndpoint::from_root(root.path().join("runtime"))?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let client = loop {
            if let Ok(client) = PwfClient::connect(&endpoint).await
                && client.check_health().await.is_ok()
            {
                break client;
            }
            let server = child.0.as_mut().context("Missing starting server")?;
            ensure!(
                server.try_wait()?.is_none(),
                "Benchmark server exited: {}",
                fs::read_to_string(root.path().join("server.log"))?
            );
            ensure!(
                Instant::now() < deadline,
                "Benchmark server readiness timed out"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        client
            .project()
            .add_project(pb::AddProjectRequest {
                fields: Some(pb::ProjectFields {
                    id: "BEN".into(),
                    title: "Resource benchmark".into(),
                    source_kind: Some("directory".into()),
                    source_value: Some(root.path().join("source").to_string_lossy().into_owned()),
                    tasks_kind: "directory".into(),
                    tasks_path: root.path().join("tasks").to_string_lossy().into_owned(),
                    obsidian_vault: None,
                    snapshot_enabled: false,
                }),
            })
            .await?;
        let fixture = Self {
            root,
            server: child.0.take().context("Missing starting server")?,
            client,
            sources,
        };
        // Prime discovery and the filesystem cache outside measured work.
        for _ in 0..RPC_REPETITIONS {
            fixture.validate_rpc(&fixture.list(pb::OrderField::Id).await?, pb::OrderField::Id)?;
        }
        // Check the CLI executable against the fixture before timing it.
        fixture.validate_cli(
            "summary",
            &fixture.command(binary, "summary").output()?.stdout,
        )?;
        Ok(fixture)
    }

    fn command(&self, binary: &Path, format: &str) -> Command {
        let mut command = Command::new(binary);
        configure(&mut command, self.root.path());
        command.args(["task", "list", "--project", "BEN", "--all"]);
        if format != "summary" {
            command.arg(format!("--long={format}"));
        }
        command
    }

    async fn list(&self, order: pb::OrderField) -> Result<Vec<pb::ListedTask>> {
        let client = self.client.task();
        let mut request = pb::ListTasksRequest {
            project_id: Some("BEN".into()),
            all: true,
            detail: pb::ListDetail::Summary as i32,
            page_size: 256,
            order: Some(pb::OrderSpec {
                field: order as i32,
                direction: if order == pb::OrderField::Title {
                    pb::OrderDirection::Asc
                } else {
                    pb::OrderDirection::Desc
                } as i32,
            }),
            ..Default::default()
        };
        let mut tasks = Vec::with_capacity(self.sources.len());
        for _ in 0..self.sources.len().div_ceil(256) {
            let mut response = client.list_tasks(request.clone()).await?;
            tasks.append(&mut response.tasks);
            let Some(token) = response.next_page_token else {
                return Ok(tasks);
            };
            request.page_token = Some(token);
        }
        anyhow::bail!("RPC listing exceeded the expected page count")
    }

    fn validate_rpc(&self, tasks: &[pb::ListedTask], order: pb::OrderField) -> Result<()> {
        ensure!(tasks.len() == self.sources.len(), "RPC task count changed");
        let mut numbers = (1..=self.sources.len()).collect::<Vec<_>>();
        if order == pb::OrderField::Title {
            numbers.sort_by_cached_key(|number| format!("Benchmark record {number}"));
        } else {
            numbers.reverse();
        }
        for (task, number) in tasks.iter().zip(numbers) {
            ensure!(
                task.id == format!("BEN-{number:04}"),
                "RPC ordering changed"
            );
        }
        Ok(())
    }

    fn validate_cli(&self, format: &str, output: &[u8]) -> Result<()> {
        let text = std::str::from_utf8(output)?;
        match format {
            "md" => {
                let expected = self
                    .sources
                    .iter()
                    .rev()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n";
                ensure!(
                    text == expected,
                    "CLI changed authored Markdown or page separators"
                );
            }
            "json" => {
                let tasks: Vec<serde_json::Value> = serde_json::from_slice(output)?;
                ensure!(
                    tasks.len() == self.sources.len(),
                    "CLI JSON task count changed"
                );
                for (index, task) in tasks.iter().enumerate() {
                    ensure!(
                        task["id"] == format!("BEN-{:04}", tasks.len() - index),
                        "CLI JSON ordering changed"
                    );
                    ensure!(
                        task["body"]
                            .as_str()
                            .is_some_and(|body| body.starts_with("## Goals\n\n")),
                        "CLI JSON omitted the body"
                    );
                }
            }
            "summary" | "rich" => {
                let headings = text
                    .lines()
                    .filter(|line| line.starts_with("BEN-"))
                    .collect::<Vec<_>>();
                ensure!(
                    headings.len() == self.sources.len(),
                    "CLI task count changed"
                );
                for (index, heading) in headings.iter().enumerate() {
                    ensure!(
                        heading.starts_with(&format!("BEN-{:04}", headings.len() - index)),
                        "CLI ordering changed"
                    );
                }
                if format == "rich" {
                    ensure!(
                        text.matches("## Goals\n\n").count() == self.sources.len(),
                        "Rich listing omitted bodies"
                    );
                }
            }
            _ => anyhow::bail!("Unknown CLI format {format}"),
        }
        Ok(())
    }

    async fn measure_rpc(&self, readers: usize, order: pb::OrderField) -> Result<Sample> {
        let started = Instant::now();
        let mut results = Vec::new();
        for _ in 0..RPC_REPETITIONS {
            results = futures::future::try_join_all((0..readers).map(|_| self.list(order))).await?;
        }
        let latency_ms = started.elapsed().as_secs_f64() * 1000.0 / f64::from(RPC_REPETITIONS);
        for tasks in &results {
            self.validate_rpc(tasks, order)?;
        }
        let (rss_resident_kib, rss_peak_kib) = resources(self.server.id())?;
        Ok(Sample {
            latency_ms,
            rss_peak_kib,
            rss_resident_kib,
        })
    }

    async fn measure_detailed_rpc(&self) -> Result<Sample> {
        let started = Instant::now();
        let mut results = Vec::new();
        for _ in 0..DETAILED_RPC_REPETITIONS {
            results = futures::future::try_join_all((0..CONCURRENT_READERS).map(|_| async {
                self.client
                    .task()
                    .list_tasks(pb::ListTasksRequest {
                        project_id: Some("BEN".into()),
                        all: true,
                        detail: pb::ListDetail::Detailed as i32,
                        page_size: 256,
                        ..Default::default()
                    })
                    .await
            }))
            .await?;
        }
        let latency_ms =
            started.elapsed().as_secs_f64() * 1000.0 / f64::from(DETAILED_RPC_REPETITIONS);
        for result in results {
            ensure!(
                result.tasks.len() == self.sources.len().min(256),
                "Detailed RPC task count changed"
            );
            for (index, task) in result.tasks.iter().enumerate() {
                ensure!(
                    task.id == format!("BEN-{:04}", self.sources.len() - index),
                    "Detailed RPC ordering changed"
                );
                ensure!(
                    task.source.as_ref() == self.sources.get(self.sources.len() - index - 1),
                    "Detailed RPC changed source bytes"
                );
            }
        }
        let (rss_resident_kib, rss_peak_kib) = resources(self.server.id())?;
        Ok(Sample {
            latency_ms,
            rss_peak_kib,
            rss_resident_kib,
        })
    }

    fn measure_cli(&self, binary: &Path, format: &str) -> Result<Sample> {
        let report_path = self.root.path().join("time.txt");
        let mut command = Command::new("/usr/bin/time");
        configure(&mut command, self.root.path());
        command
            .args(["-f", "%M", "-o"])
            .arg(&report_path)
            .arg(binary);
        command.args(self.command(binary, format).get_args());
        let started = Instant::now();
        let output = command.output()?;
        let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
        ensure!(
            output.status.success(),
            "CLI benchmark failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        ensure!(
            output.stderr.is_empty(),
            "CLI benchmark wrote unexpected stderr"
        );
        let rss_peak_kib = fs::read_to_string(report_path)?.trim().parse()?;
        self.validate_cli(format, &output.stdout)?;
        Ok(Sample {
            latency_ms,
            rss_peak_kib,
            rss_resident_kib: 0,
        })
    }
}

struct ServerStarting(Option<Child>);

impl Drop for ServerStarting {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
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
        .env("NO_COLOR", "1")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("FORCE_COLOR");
}

fn resources(pid: u32) -> Result<(u32, u32)> {
    let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
    let field = |name: &str| -> Result<u32> {
        Ok(status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .context("Missing process memory counter")?
            .split_whitespace()
            .next()
            .context("Empty process memory counter")?
            .parse()?)
    };
    Ok((field("VmRSS:")?, field("VmHWM:")?))
}

fn command_text(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    ensure!(output.status.success(), "Benchmark metadata command failed");
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn binary_hash(binary: &Path) -> Result<String> {
    Ok(command_text(Command::new("sha256sum").arg(binary))?
        .split_whitespace()
        .next()
        .context("Missing binary hash")?
        .into())
}

fn configuration() -> Result<Configuration> {
    Ok(Configuration {
        schema: 3,
        profile: "release/warm-filesystem/watched-index".into(),
        os: command_text(Command::new("uname").arg("-sr"))?,
        cpu: fs::read_to_string("/proc/cpuinfo")?
            .lines()
            .find_map(|line| line.strip_prefix("model name\t: "))
            .context("Missing CPU model")?
            .into(),
        cpus: std::thread::available_parallelism()?.get(),
        rustc: command_text(Command::new("rustc").arg("-V"))?,
        samples: SAMPLES,
        rpc_repetitions: RPC_REPETITIONS,
        detailed_rpc_repetitions: DETAILED_RPC_REPETITIONS,
        concurrent_readers: CONCURRENT_READERS,
        logging: "info/stderr-file/rotating-jsonl".into(),
        cli_runner: command_text(Command::new("/usr/bin/time").arg("--version"))?,
        workloads: WORKLOADS
            .iter()
            .map(|&(name, records, bytes)| (name.into(), records, bytes))
            .collect(),
        formats: FORMATS.iter().map(|&format| format.into()).collect(),
    })
}

fn write_report(path: &Path, report: &Report) -> Result<()> {
    let mut staged =
        tempfile::NamedTempFile::new_in(path.parent().context("Report has no parent")?)?;
    serde_json::to_writer_pretty(staged.as_file_mut(), report)?;
    staged.persist(path)?;
    Ok(())
}

fn median(values: impl Iterator<Item = f64>) -> f64 {
    let mut values = values.collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn read_baseline(
    path: &Path,
    configuration: &Configuration,
    update: bool,
) -> Result<Option<Report>> {
    if !path.exists() {
        ensure!(
            update,
            "Server/CLI baseline is missing; capture it with --update"
        );
        return Ok(None);
    }
    let decoded = serde_json::from_slice::<Report>(&fs::read(path)?);
    let report = match decoded {
        Ok(report) => report,
        Err(error) if update => {
            eprintln!("Replacing an incompatible server/CLI baseline: {error}");
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let expected = WORKLOADS
        .iter()
        .flat_map(|&(name, _, _)| {
            let mut cases = vec![
                (format!("{name}/rpc-1-readers"), SAMPLES),
                (format!("{name}/rpc-{CONCURRENT_READERS}-readers"), SAMPLES),
                (format!("{name}/rpc-title-1-readers"), SAMPLES),
                (
                    format!("{name}/rpc-title-{CONCURRENT_READERS}-readers"),
                    SAMPLES,
                ),
                (
                    format!("{name}/rpc-detailed-first-page-{CONCURRENT_READERS}-readers"),
                    SAMPLES,
                ),
            ];
            cases.extend(
                FORMATS
                    .iter()
                    .map(|format| (format!("{name}/cli-{format}"), SAMPLES)),
            );
            cases.push((format!("{name}/server-after-cli"), 1));
            cases
        })
        .collect::<Vec<_>>();
    let compatible = report.configuration == *configuration
        && report.cases.len() == expected.len()
        && report
            .cases
            .iter()
            .zip(expected)
            .all(|(case, (name, count))| {
                case.name == name
                    && case.samples.len() == count
                    && case.samples.iter().all(|sample| {
                        sample.latency_ms.is_finite()
                            && sample.latency_ms >= 0.0
                            && sample.rss_peak_kib > 0
                    })
            });
    if compatible {
        return Ok(Some(report));
    }
    ensure!(
        update,
        "Server/CLI baseline is incompatible; capture a fresh baseline with --update"
    );
    Ok(None)
}

fn freeze_binary(binary: &Path, path: &Path) -> Result<()> {
    let staged = tempfile::NamedTempFile::new_in(path.parent().context("Binary has no parent")?)?;
    fs::copy(binary, staged.path())?;
    staged.persist(path)?;
    Ok(())
}

fn measure_workload(
    runtime: &tokio::runtime::Runtime,
    fixture: &Fixture,
    binary: &Path,
    name: &str,
) -> Result<Vec<Case>> {
    let mut cases = Vec::new();
    for (order, order_name) in [(pb::OrderField::Id, ""), (pb::OrderField::Title, "title-")] {
        for readers in [1, CONCURRENT_READERS] {
            let case_name = format!("{name}/rpc-{order_name}{readers}-readers");
            let mut samples = Vec::new();
            for _ in 0..SAMPLES {
                samples.push(runtime.block_on(fixture.measure_rpc(readers, order))?);
            }
            println!(
                "{case_name}: {:.2} ms, {} KiB server RSS",
                median(samples.iter().map(|sample| sample.latency_ms)),
                samples.last().context("Missing sample")?.rss_resident_kib
            );
            cases.push(Case {
                name: case_name,
                samples,
            });
        }
    }
    let mut samples = Vec::new();
    for _ in 0..SAMPLES {
        samples.push(runtime.block_on(fixture.measure_detailed_rpc())?);
    }
    let case_name = format!("{name}/rpc-detailed-first-page-{CONCURRENT_READERS}-readers");
    println!(
        "{case_name}: {:.2} ms, {} KiB server RSS",
        median(samples.iter().map(|sample| sample.latency_ms)),
        samples.last().context("Missing sample")?.rss_resident_kib
    );
    cases.push(Case {
        name: case_name,
        samples,
    });
    for format in FORMATS {
        let mut samples = Vec::new();
        for _ in 0..SAMPLES {
            samples.push(fixture.measure_cli(binary, format)?);
        }
        let case_name = format!("{name}/cli-{format}");
        println!(
            "{case_name}: {:.2} ms, {:.0} KiB CLI peak RSS",
            median(samples.iter().map(|sample| sample.latency_ms)),
            median(samples.iter().map(|sample| f64::from(sample.rss_peak_kib)))
        );
        cases.push(Case {
            name: case_name,
            samples,
        });
    }
    let (rss_resident_kib, rss_peak_kib) = resources(fixture.server.id())?;
    cases.push(Case {
        name: format!("{name}/server-after-cli"),
        samples: vec![Sample {
            latency_ms: 0.0,
            rss_peak_kib,
            rss_resident_kib,
        }],
    });
    Ok(cases)
}

fn compare(before: &Report, after: &Report) {
    for (before, after) in before.cases.iter().zip(&after.cases) {
        let latency_before = median(before.samples.iter().map(|sample| sample.latency_ms));
        let latency_after = median(after.samples.iter().map(|sample| sample.latency_ms));
        let rss_before = median(
            before
                .samples
                .iter()
                .map(|sample| f64::from(sample.rss_peak_kib)),
        );
        let rss_after = median(
            after
                .samples
                .iter()
                .map(|sample| f64::from(sample.rss_peak_kib)),
        );
        if latency_before > 0.0 {
            println!(
                "{}: latency {latency_before:.2} -> {latency_after:.2} ms ({:+.1}%), peak RSS {rss_before:.0} -> {rss_after:.0} KiB ({:+.1}%)",
                after.name,
                (latency_after / latency_before - 1.0) * 100.0,
                (rss_after / rss_before - 1.0) * 100.0
            );
        } else {
            println!(
                "{}: peak RSS {rss_before:.0} -> {rss_after:.0} KiB ({:+.1}%)",
                after.name,
                (rss_after / rss_before - 1.0) * 100.0
            );
        }
    }
}

pub(super) fn run() -> Result<()> {
    let arguments = Arguments::parse()?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = arguments
        .binary
        .unwrap_or_else(|| root.join("target/release/pwf"));
    let server = arguments
        .server
        .unwrap_or_else(|| root.join("target/release/pwf-server"));
    let artifact = root.join(".artifacts/benchmarks/server-cli");
    fs::create_dir_all(&artifact)?;
    let configuration = configuration()?;
    let baseline_path = artifact.join("baseline.json");
    let baseline = read_baseline(&baseline_path, &configuration, arguments.update)?;
    let measured_binary_hash = binary_hash(&binary)?;
    let measured_server_hash = binary_hash(&server)?;
    let load_average_start = Some(load_average()?);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut cases = Vec::new();
    for (name, records, body_bytes) in WORKLOADS {
        let fixture = runtime.block_on(Fixture::new(&binary, &server, records, body_bytes))?;
        cases.extend(measure_workload(&runtime, &fixture, &binary, name)?);
    }
    ensure!(
        measured_binary_hash == binary_hash(&binary)?
            && measured_server_hash == binary_hash(&server)?,
        "A measured executable changed during the benchmark"
    );
    let report = Report {
        configuration,
        binary_hash: measured_binary_hash,
        server_hash: measured_server_hash,
        load_average_start,
        load_average_end: Some(load_average()?),
        cases,
    };
    write_report(&artifact.join("current.json"), &report)?;
    if let Some(baseline) = baseline {
        compare(&baseline, &report);
    }
    if arguments.update {
        freeze_binary(&binary, &artifact.join("pwf-baseline"))?;
        freeze_binary(&server, &artifact.join("pwf-server-baseline"))?;
        write_report(&baseline_path, &report)?;
    }
    Ok(())
}

fn load_average() -> Result<[f64; 3]> {
    let load = fs::read_to_string("/proc/loadavg")?;
    let mut fields = load.split_whitespace();
    let mut values = [0.0; 3];
    for value in &mut values {
        *value = fields.next().context("Missing load average")?.parse()?;
    }
    Ok(values)
}
