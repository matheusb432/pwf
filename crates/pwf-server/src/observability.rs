use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use directories::BaseDirs;
use rolling_file::{BasicRollingFileAppender, RollingConditionBasic};
use tracing_appender::non_blocking::{ErrorCounter, NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _};

const SEGMENT_SIZE_BYTES: u64 = 8 * 1024 * 1024;
const HISTORICAL_SEGMENT_COUNT: usize = 15;
const QUEUED_RECORD_COUNT: usize = 4_096;
const FILE_NAME: &str = "pwf-server.jsonl";

#[derive(Debug)]
struct ObservabilitySettings {
    directory: PathBuf,
    segment_size_bytes: u64,
    historical_segment_count: usize,
    queued_record_count: usize,
}

impl ObservabilitySettings {
    fn production(directory: PathBuf) -> Self {
        Self {
            directory,
            segment_size_bytes: SEGMENT_SIZE_BYTES,
            historical_segment_count: HISTORICAL_SEGMENT_COUNT,
            queued_record_count: QUEUED_RECORD_COUNT,
        }
    }
}

pub(crate) struct ObservabilityGuard {
    worker: Option<WorkerGuard>,
    dropped_records: ErrorCounter,
}

impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        drop(self.worker.take());
        let dropped_record_count = self.dropped_records.dropped_lines();
        if dropped_record_count != 0 {
            eprintln!("pwf-server observability dropped {dropped_record_count} JSONL records");
        }
    }
}

pub(crate) fn initialize() -> anyhow::Result<ObservabilityGuard> {
    let filter = filter_from_env()?;
    let settings = ObservabilitySettings::production(default_log_directory()?);
    let (dispatch, guard) = build_dispatch(&settings, filter)?;
    tracing::dispatcher::set_global_default(dispatch)
        .context("installing the pwf-server tracing dispatcher")?;
    Ok(guard)
}

fn filter_from_env() -> anyhow::Result<EnvFilter> {
    if env::var_os("RUST_LOG").is_some() {
        EnvFilter::try_from_default_env().context("parsing RUST_LOG")
    } else {
        Ok(EnvFilter::new("info"))
    }
}

fn default_log_directory() -> anyhow::Result<PathBuf> {
    let base_directories = BaseDirs::new().context("resolving platform-local directories")?;
    Ok(log_directory(
        base_directories.state_dir(),
        base_directories.data_local_dir(),
    ))
}

fn log_directory(state_directory: Option<&Path>, local_data_directory: &Path) -> PathBuf {
    state_directory
        .unwrap_or(local_data_directory)
        .join("pwf/logs")
}

fn build_dispatch(
    settings: &ObservabilitySettings,
    filter: EnvFilter,
) -> anyhow::Result<(tracing::Dispatch, ObservabilityGuard)> {
    fs::create_dir_all(&settings.directory).with_context(|| {
        format!(
            "creating observability directory {}",
            settings.directory.display()
        )
    })?;
    let file_path = settings.directory.join(FILE_NAME);
    let rolling_writer = BasicRollingFileAppender::new(
        &file_path,
        RollingConditionBasic::new().max_size(settings.segment_size_bytes),
        settings.historical_segment_count,
    )
    .with_context(|| {
        format!(
            "opening the pwf-server observability file {}",
            file_path.display()
        )
    })?;
    let reporting_writer = RuntimeErrorReportingWriter::new(rolling_writer);
    let (file_writer, worker, dropped_records) =
        build_non_blocking_writer(reporting_writer, settings.queued_record_count);

    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(io::stderr);
    let json_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .with_writer(file_writer);
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(json_layer);

    Ok((
        tracing::Dispatch::new(subscriber),
        ObservabilityGuard {
            worker: Some(worker),
            dropped_records,
        },
    ))
}

fn build_non_blocking_writer<Writer>(
    writer: Writer,
    queued_record_count: usize,
) -> (NonBlocking, WorkerGuard, ErrorCounter)
where
    Writer: Write + Send + 'static,
{
    let (writer, worker) = NonBlockingBuilder::default()
        .buffered_lines_limit(queued_record_count)
        .lossy(true)
        .thread_name("pwf-server-observability")
        .finish(writer);
    let dropped_records = writer.error_counter();
    (writer, worker, dropped_records)
}

struct RuntimeErrorReportingWriter<Writer, Reporter = fn(&str)> {
    writer: Writer,
    reporter: Reporter,
    failure_active: bool,
}

impl<Writer> RuntimeErrorReportingWriter<Writer, fn(&str)> {
    fn new(writer: Writer) -> Self {
        Self::with_reporter(writer, report_runtime_error)
    }
}

impl<Writer, Reporter> RuntimeErrorReportingWriter<Writer, Reporter> {
    fn with_reporter(writer: Writer, reporter: Reporter) -> Self {
        Self {
            writer,
            reporter,
            failure_active: false,
        }
    }
}

impl<Writer, Reporter> RuntimeErrorReportingWriter<Writer, Reporter>
where
    Reporter: FnMut(&str),
{
    fn observe_result<T>(&mut self, operation: &str, result: &io::Result<T>) {
        match result {
            Err(error) if !self.failure_active => {
                self.failure_active = true;
                let message = format!("pwf-server observability {operation} failed: {error}");
                (self.reporter)(&message);
            }
            Ok(_) if self.failure_active => {
                self.failure_active = false;
                (self.reporter)("pwf-server observability writer recovered");
            }
            Ok(_) | Err(_) => {}
        }
    }
}

impl<Writer, Reporter> Write for RuntimeErrorReportingWriter<Writer, Reporter>
where
    Writer: Write,
    Reporter: FnMut(&str),
{
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let result = self.writer.write(buffer);
        self.observe_result("write", &result);
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        let result = self.writer.flush();
        self.observe_result("flush", &result);
        result
    }
}

fn report_runtime_error(message: &str) {
    eprintln!("{message}");
}

#[cfg(test)]
pub(crate) fn read_json_records(directory: &Path) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut records = Vec::new();
    for path in jsonl_paths(directory)? {
        for line in fs::read_to_string(path)?
            .lines()
            .filter(|line| !line.is_empty())
        {
            records.push(serde_json::from_str(line)?);
        }
    }
    Ok(records)
}

#[cfg(test)]
fn jsonl_paths(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    paths.retain(|path| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(FILE_NAME))
    });
    paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::VecDeque,
        fs,
        io::{self, Write as _},
        path::{Path, PathBuf},
        rc::Rc,
        sync::mpsc,
        time::Duration,
    };

    use tracing_subscriber::EnvFilter;

    use super::{
        FILE_NAME, ObservabilitySettings, RuntimeErrorReportingWriter, build_dispatch,
        build_non_blocking_writer, jsonl_paths, log_directory, read_json_records,
    };

    type TestResult<T = ()> = anyhow::Result<T>;

    #[test]
    fn resolves_log_directory_from_the_state_root() {
        let state_directory = Path::new("/state");
        let local_data_directory = Path::new("/local-data");

        assert_eq!(
            log_directory(Some(state_directory), local_data_directory),
            PathBuf::from("/state/pwf/logs")
        );
    }

    #[test]
    fn falls_back_to_the_local_data_root() {
        assert_eq!(
            log_directory(None, Path::new("/local-data")),
            PathBuf::from("/local-data/pwf/logs")
        );
    }

    #[test]
    fn writes_inspectable_nested_json_records() -> TestResult {
        let directory = tempfile::tempdir()?;
        let settings = test_settings(directory.path().join("logs"), 1024 * 1024, 2, 64);
        let (dispatch, guard) = build_dispatch(&settings, EnvFilter::new("trace"))?;

        tracing::dispatcher::with_default(&dispatch, || {
            let outer = tracing::info_span!("outer", outer_field = "outer-value");
            let _outer_entered = outer.enter();
            let inner = tracing::info_span!("inner", inner_field = 11);
            let _inner_entered = inner.enter();
            tracing::info!(
                target: "pwf_server::observability_test",
                record_index = 7,
                "structured record"
            );
        });
        drop(guard);

        let records = read_json_records(&settings.directory)?;
        let record = records
            .iter()
            .find(|record| record["fields"]["message"] == "structured record")
            .ok_or_else(|| anyhow::anyhow!("durable sink omitted the structured record"))?;
        assert!(
            record["timestamp"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        assert_eq!(record["level"], "INFO");
        assert_eq!(record["target"], "pwf_server::observability_test");
        assert_eq!(record["fields"]["record_index"], 7);
        assert_eq!(record["span"]["name"], "inner");
        assert_eq!(record["span"]["inner_field"], 11);
        let span_names = record["spans"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("durable record omitted its span list"))?
            .iter()
            .map(|span| span["name"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(span_names, ["outer", "inner"]);
        assert_eq!(record["spans"][0]["outer_field"], "outer-value");
        Ok(())
    }

    #[test]
    fn rotates_complete_json_records_with_bounded_history() -> TestResult {
        let directory = tempfile::tempdir()?;
        let settings = test_settings(directory.path().join("logs"), 512, 2, 256);
        let (dispatch, guard) = build_dispatch(&settings, EnvFilter::new("trace"))?;
        let payload = "x".repeat(256);

        tracing::dispatcher::with_default(&dispatch, || {
            for record_index in 0..8 {
                tracing::info!(
                    target: "pwf_server::rotation_test",
                    record_index,
                    payload,
                    "rotation record"
                );
            }
        });
        drop(guard);

        let paths = jsonl_paths(&settings.directory)?;
        let file_names = paths
            .iter()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            file_names,
            [
                FILE_NAME.to_owned(),
                format!("{FILE_NAME}.1"),
                format!("{FILE_NAME}.2"),
            ]
        );

        let records = read_json_records(&settings.directory)?;
        assert!(
            records
                .iter()
                .any(|record| record["fields"]["record_index"] == 7)
        );
        assert!(
            records
                .iter()
                .all(|record| record["fields"]["record_index"] != 0)
        );
        Ok(())
    }

    #[test]
    fn fails_before_installation_when_the_log_directory_is_a_file() -> TestResult {
        let directory = tempfile::tempdir()?;
        let log_directory = directory.path().join("blocked");
        fs::write(&log_directory, b"not a directory")?;
        let settings = test_settings(log_directory.clone(), 1024, 1, 16);

        let Err(error) = build_dispatch(&settings, EnvFilter::new("trace")) else {
            return Err(anyhow::anyhow!(
                "dispatcher construction unexpectedly succeeded"
            ));
        };
        let message = format!("{error:#}");
        assert!(message.contains("creating observability directory"));
        assert!(message.contains(&log_directory.display().to_string()));
        Ok(())
    }

    #[test]
    fn drops_records_instead_of_blocking_when_the_queue_is_full() -> TestResult {
        let (write_started_sender, write_started_receiver) = mpsc::sync_channel(1);
        let (write_release_sender, write_release_receiver) = mpsc::sync_channel(1);
        let gate_writer = GateWriter {
            write_started_sender,
            write_release_receiver,
            gate_next_write: true,
        };
        let (mut writer, worker_guard, dropped_records) = build_non_blocking_writer(gate_writer, 1);

        writer.write_all(b"worker-blocking record")?;
        write_started_receiver.recv_timeout(Duration::from_secs(1))?;
        writer.write_all(b"queued record")?;
        writer.write_all(b"dropped record")?;
        assert_eq!(dropped_records.dropped_lines(), 1);

        write_release_sender.send(())?;
        drop(writer);
        drop(worker_guard);
        Ok(())
    }

    #[test]
    fn reports_runtime_failure_transitions_once() -> TestResult {
        let reports = Rc::new(RefCell::new(Vec::new()));
        let captured_reports = Rc::clone(&reports);
        let scripted_writer = ScriptedWriter::new([false, false, true, false]);
        let mut writer =
            RuntimeErrorReportingWriter::with_reporter(scripted_writer, move |message: &str| {
                captured_reports.borrow_mut().push(message.to_owned());
            });

        assert!(writer.write_all(b"first failure").is_err());
        assert!(writer.flush().is_err());
        writer.write_all(b"recovered")?;
        assert!(writer.flush().is_err());

        assert_eq!(
            reports.borrow().as_slice(),
            [
                "pwf-server observability write failed: planned failure",
                "pwf-server observability writer recovered",
                "pwf-server observability flush failed: planned failure",
            ]
        );
        Ok(())
    }

    fn test_settings(
        directory: PathBuf,
        segment_size_bytes: u64,
        historical_segment_count: usize,
        queued_record_count: usize,
    ) -> ObservabilitySettings {
        ObservabilitySettings {
            directory,
            segment_size_bytes,
            historical_segment_count,
            queued_record_count,
        }
    }

    struct GateWriter {
        write_started_sender: mpsc::SyncSender<()>,
        write_release_receiver: mpsc::Receiver<()>,
        gate_next_write: bool,
    }

    impl io::Write for GateWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if self.gate_next_write {
                self.gate_next_write = false;
                self.write_started_sender
                    .send(())
                    .map_err(|_| io::Error::other("write-start receiver closed"))?;
                self.write_release_receiver
                    .recv()
                    .map_err(|_| io::Error::other("write-release sender closed"))?;
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct ScriptedWriter {
        operation_successes: VecDeque<bool>,
    }

    impl ScriptedWriter {
        fn new(operation_successes: impl IntoIterator<Item = bool>) -> Self {
            Self {
                operation_successes: operation_successes.into_iter().collect(),
            }
        }

        fn next_result(&mut self) -> io::Result<()> {
            match self.operation_successes.pop_front() {
                Some(true) => Ok(()),
                Some(false) => Err(io::Error::other("planned failure")),
                None => Err(io::Error::other("script exhausted")),
            }
        }
    }

    impl io::Write for ScriptedWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.next_result().map(|()| buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.next_result()
        }
    }
}
