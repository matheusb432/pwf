use std::{
    error::Error,
    fmt,
    process::{Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
#[cfg(windows)]
use process_wrap::std::JobObject;
#[cfg(unix)]
use process_wrap::std::ProcessGroup;
use process_wrap::std::{ChildWrapper, CommandWrap};

const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const TERMINATION_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct TimeoutError {
    label: String,
    deadline: Duration,
}

impl fmt::Display for TimeoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} timed out after {}",
            self.label,
            duration_label(self.deadline)
        )
    }
}

impl Error for TimeoutError {}

pub(crate) fn run(command: Command, label: &str, deadline: Duration) -> Result<ExitStatus> {
    let program = command.get_program().to_string_lossy().into_owned();
    let mut command = CommandWrap::from(command);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);
    let mut child = command
        .spawn()
        .with_context(|| format!("spawning {label} from {program}"))?;
    let status = match wait_for(child.as_mut(), deadline) {
        Ok(status) => status,
        Err(error) => {
            terminate_and_reap(child.as_mut(), label).with_context(|| {
                format!("waiting for {label} failed ({error}); terminating process tree")
            })?;
            return Err(error).with_context(|| format!("waiting for {label} from {program}"));
        }
    };

    if let Some(status) = status {
        return Ok(status);
    }

    terminate_and_reap(child.as_mut(), label).with_context(|| {
        format!(
            "{label} timed out after {}; terminating process tree",
            duration_label(deadline)
        )
    })?;
    Err(TimeoutError {
        label: label.to_owned(),
        deadline,
    }
    .into())
}

pub(crate) fn is_timeout(error: &anyhow::Error) -> bool {
    error.downcast_ref::<TimeoutError>().is_some()
}

fn wait_for(
    child: &mut dyn ChildWrapper,
    duration: Duration,
) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + duration;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }

        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        thread::sleep(STATUS_POLL_INTERVAL.min(deadline.duration_since(now)));
    }
}

fn terminate_and_reap(child: &mut dyn ChildWrapper, label: &str) -> Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }

    if let Err(error) = child.start_kill() {
        if child.try_wait()?.is_none() {
            return Err(error).with_context(|| format!("terminating {label}"));
        }
        return Ok(());
    }

    if wait_for(child, TERMINATION_WAIT)
        .with_context(|| format!("reaping {label} after termination"))?
        .is_none()
    {
        bail!(
            "{label} did not exit within {} after termination",
            duration_label(TERMINATION_WAIT)
        );
    }
    Ok(())
}

fn duration_label(duration: Duration) -> String {
    if duration.subsec_nanos() == 0 {
        format!("{} s", duration.as_secs())
    } else {
        format!("{} ms", duration.as_millis())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::TcpListener,
        path::Path,
        process::Command,
        thread,
        time::{Duration, Instant},
    };

    use super::*;

    const GRANDCHILD_READY_ENVIRONMENT: &str = "PWF_XTASK_GRANDCHILD_READY";
    const SLEEPING_PARENT_ENVIRONMENT: &str = "PWF_XTASK_SLEEPING_PARENT";

    #[test]
    fn deadline_terminates_and_reaps_a_sleeping_process_tree() {
        let directory = tempfile::tempdir().expect("temporary fixture directory");
        let ready_path = directory.path().join("grandchild-port");
        let executable = std::env::current_exe().expect("current test executable");
        let mut command = Command::new(executable);
        command
            .args([
                "--exact",
                "child_process::tests::sleeping_parent_stub",
                "--ignored",
            ])
            .env(SLEEPING_PARENT_ENVIRONMENT, "1")
            .env(GRANDCHILD_READY_ENVIRONMENT, &ready_path);

        let started = Instant::now();
        let error = run(command, "sleeping process tree", Duration::from_secs(2))
            .expect_err("sleeping process tree must time out");

        assert!(
            error
                .to_string()
                .contains("sleeping process tree timed out after 2 s")
        );
        assert!(started.elapsed() < Duration::from_secs(8));

        let port = std::fs::read_to_string(&ready_path)
            .expect("sleeping grandchild readiness")
            .parse::<u16>()
            .expect("sleeping grandchild port");
        let address = ("127.0.0.1", port);
        let release_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match TcpListener::bind(address) {
                Ok(_) => break,
                Err(_error) if Instant::now() < release_deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("sleeping grandchild remains alive: {error}"),
            }
        }
    }

    #[test]
    #[ignore = "child-process timeout fixture"]
    fn sleeping_parent_stub() {
        if std::env::var_os(SLEEPING_PARENT_ENVIRONMENT).is_none() {
            return;
        }

        let ready_path =
            std::env::var_os(GRANDCHILD_READY_ENVIRONMENT).expect("grandchild readiness path");
        let executable = std::env::current_exe().expect("current test executable");
        let mut grandchild = Command::new(executable)
            .args([
                "--exact",
                "child_process::tests::sleeping_grandchild_stub",
                "--ignored",
            ])
            .env(GRANDCHILD_READY_ENVIRONMENT, &ready_path)
            .spawn()
            .expect("sleeping grandchild");

        let readiness_deadline = Instant::now() + Duration::from_secs(5);
        while !Path::new(&ready_path).is_file() {
            assert!(
                Instant::now() < readiness_deadline,
                "sleeping grandchild did not become ready"
            );
            thread::sleep(Duration::from_millis(10));
        }

        thread::sleep(Duration::from_secs(10));
        let _ = grandchild.kill();
        let _ = grandchild.wait();
    }

    #[test]
    #[ignore = "child-process timeout fixture"]
    fn sleeping_grandchild_stub() {
        let Some(ready_path) = std::env::var_os(GRANDCHILD_READY_ENVIRONMENT) else {
            return;
        };
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("grandchild listener");
        let port = listener.local_addr().expect("grandchild address").port();
        std::fs::write(ready_path, port.to_string()).expect("grandchild readiness");

        thread::sleep(Duration::from_secs(10));
    }
}
