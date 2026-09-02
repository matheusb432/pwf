use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use allocation_counter::AllocationInfo;
use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

const REPORT_SCHEMA_VERSION: u32 = 1;

pub struct AllocationCase {
    name: String,
    measure: Box<dyn Fn() -> AllocationInfo>,
}

impl AllocationCase {
    pub fn new(name: impl Into<String>, measure: impl Fn() -> AllocationInfo + 'static) -> Self {
        Self {
            name: name.into(),
            measure: Box::new(measure),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Compatibility {
    schema_version: u32,
    benchmark: String,
    fixture_schema_version: u32,
    allocation_sample_count: usize,
    cases: Vec<String>,
    rustc: String,
    target: String,
    profile: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Report {
    compatibility: Compatibility,
    source_revision: String,
    source_dirty: bool,
    cases: Vec<CaseReport>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CaseReport {
    name: String,
    samples: Vec<AllocationSample>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct AllocationSample {
    count_total: u64,
    count_current: i64,
    count_max: u64,
    bytes_total: u64,
    bytes_current: i64,
    bytes_max: u64,
}

impl From<AllocationInfo> for AllocationSample {
    fn from(info: AllocationInfo) -> Self {
        Self {
            count_total: info.count_total,
            count_current: info.count_current,
            count_max: info.count_max,
            bytes_total: info.bytes_total,
            bytes_current: info.bytes_current,
            bytes_max: info.bytes_max,
        }
    }
}

#[derive(Clone, Copy)]
struct AllocationMedian {
    count_total: u64,
    bytes_total: u64,
    bytes_max: u64,
}

pub fn run(
    benchmark: &str,
    fixture_schema_version: u32,
    allocation_sample_count: usize,
    cases: Vec<AllocationCase>,
) -> Result<()> {
    let update = parse_arguments()?;
    let compatibility = compatibility(
        benchmark,
        fixture_schema_version,
        allocation_sample_count,
        &cases,
    )?;
    let report_directory = workspace_root()
        .join(".artifacts/benchmarks")
        .join(benchmark)
        .join("allocations");
    let current_path = report_directory.join("current.json");
    let baseline_path = report_directory.join("baseline.json");
    let baseline = read_baseline(&baseline_path, update, &compatibility)?;

    for case in &cases {
        let _ = (case.measure)();
    }

    let mut case_reports = Vec::with_capacity(cases.len());
    for case in cases {
        let samples = (0..allocation_sample_count)
            .map(|_| (case.measure)().into())
            .collect();
        case_reports.push(CaseReport {
            name: case.name,
            samples,
        });
    }

    let workspace_root = workspace_root();
    let report = Report {
        compatibility,
        source_revision: git_output(&workspace_root, &["rev-parse", "HEAD"])
            .unwrap_or_else(|_| "unknown".to_string()),
        source_dirty: git_output(&workspace_root, &["status", "--porcelain"])
            .is_ok_and(|status| !status.is_empty()),
        cases: case_reports,
    };
    write_report_atomic(&current_path, &report)?;

    if let Some(baseline) = &baseline {
        print_comparison(baseline, &report)?;
    } else {
        println!("establishing allocation baseline for {benchmark}");
    }

    if update {
        write_report_atomic(&baseline_path, &report)?;
        println!("updated {}", baseline_path.display());
    } else {
        println!("wrote {}", current_path.display());
    }
    Ok(())
}

fn parse_arguments() -> Result<bool> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => Ok(false),
        [argument] if argument == "--update" => Ok(true),
        _ => bail!("expected no arguments or --update"),
    }
}

fn compatibility(
    benchmark: &str,
    fixture_schema_version: u32,
    allocation_sample_count: usize,
    cases: &[AllocationCase],
) -> Result<Compatibility> {
    Ok(Compatibility {
        schema_version: REPORT_SCHEMA_VERSION,
        benchmark: benchmark.to_string(),
        fixture_schema_version,
        allocation_sample_count,
        cases: cases.iter().map(|case| case.name.clone()).collect(),
        rustc: command_output(Command::new("rustc").args(["--version", "--verbose"]))?,
        target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        profile: "release".to_string(),
    })
}

fn read_baseline(
    path: &Path,
    update: bool,
    compatibility: &Compatibility,
) -> Result<Option<Report>> {
    if !path.exists() {
        if update {
            return Ok(None);
        }
        bail!(
            "allocation baseline is missing at {}; run just bench-allocations --update",
            path.display()
        );
    }
    let report: Report = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read allocation baseline {}", path.display()))?,
    )
    .with_context(|| format!("parse allocation baseline {}", path.display()))?;
    if report.compatibility != *compatibility {
        bail!(
            "allocation baseline at {} is incompatible with the current workload",
            path.display()
        );
    }
    Ok(Some(report))
}

fn write_report_atomic(path: &Path, report: &Report) -> Result<()> {
    let directory = path
        .parent()
        .context("allocation report path has no parent")?;
    fs::create_dir_all(directory)
        .with_context(|| format!("create allocation report directory {}", directory.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory).with_context(|| {
        format!(
            "create temporary allocation report in {}",
            directory.display()
        )
    })?;
    serde_json::to_writer_pretty(&mut temporary, report)
        .with_context(|| format!("serialize allocation report {}", path.display()))?;
    temporary
        .write_all(b"\n")
        .with_context(|| format!("finish allocation report {}", path.display()))?;
    temporary
        .as_file_mut()
        .sync_all()
        .with_context(|| format!("sync allocation report {}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("persist allocation report {}", path.display()))?;
    Ok(())
}

fn print_comparison(baseline: &Report, current: &Report) -> Result<()> {
    for current_case in &current.cases {
        let baseline_case = baseline
            .cases
            .iter()
            .find(|case| case.name == current_case.name)
            .with_context(|| format!("baseline case {} is missing", current_case.name))?;
        let baseline_median = median(&baseline_case.samples);
        let current_median = median(&current_case.samples);
        println!(
            "{} allocations {} -> {} ({}), bytes {} -> {} ({}), peak {} -> {} ({})",
            current_case.name,
            baseline_median.count_total,
            current_median.count_total,
            relative_change(baseline_median.count_total, current_median.count_total),
            baseline_median.bytes_total,
            current_median.bytes_total,
            relative_change(baseline_median.bytes_total, current_median.bytes_total),
            baseline_median.bytes_max,
            current_median.bytes_max,
            relative_change(baseline_median.bytes_max, current_median.bytes_max),
        );
    }
    Ok(())
}

fn median(samples: &[AllocationSample]) -> AllocationMedian {
    AllocationMedian {
        count_total: median_u64(samples.iter().map(|sample| sample.count_total)),
        bytes_total: median_u64(samples.iter().map(|sample| sample.bytes_total)),
        bytes_max: median_u64(samples.iter().map(|sample| sample.bytes_max)),
    }
}

fn median_u64(values: impl IntoIterator<Item = u64>) -> u64 {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_unstable();
    if values.is_empty() {
        return 0;
    }
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        values[middle - 1].saturating_add(values[middle]) / 2
    } else {
        values[middle]
    }
}

fn relative_change(baseline: u64, current: u64) -> String {
    if baseline == 0 {
        return if current == 0 {
            "+0.00%".to_string()
        } else {
            "n/a".to_string()
        };
    }
    let delta = i128::from(current) - i128::from(baseline);
    let basis_points = delta * 10_000 / i128::from(baseline);
    let sign = if basis_points < 0 { '-' } else { '+' };
    let absolute = basis_points.abs();
    format!("{sign}{}.{:02}%", absolute / 100, absolute % 100)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn git_output(workspace_root: &Path, arguments: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(workspace_root).args(arguments);
    command_output(&mut command)
}

fn command_output(command: &mut Command) -> Result<String> {
    let output = command.output().context("run benchmark metadata command")?;
    if !output.status.success() {
        bail!("benchmark metadata command failed with {}", output.status);
    }
    String::from_utf8(output.stdout)
        .context("benchmark metadata command output is not UTF-8")
        .map(|output| output.trim().to_string())
}
