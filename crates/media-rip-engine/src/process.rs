use std::{
    io,
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

const DEFAULT_TERMINATION_GRACE: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Owns a child process and guarantees that it is reaped before being dropped.
///
/// Cancellation first requests a normal termination so tools can release their
/// input devices and flush diagnostics. A process that does not exit within the
/// grace period is forcibly killed.
pub(crate) struct ProcessSupervisor {
    child: Child,
    termination_grace: Duration,
    reaped: bool,
}

impl ProcessSupervisor {
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        command.spawn().map(|child| Self {
            child,
            termination_grace: DEFAULT_TERMINATION_GRACE,
            reaped: false,
        })
    }

    pub(crate) fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    pub(crate) fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped = true;
        }
        Ok(status)
    }

    pub(crate) fn terminate(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.try_wait()? {
            return Ok(status);
        }

        if request_graceful_termination(&mut self.child).is_ok() {
            let deadline = Instant::now() + self.termination_grace;
            while Instant::now() < deadline {
                if let Some(status) = self.try_wait()? {
                    return Ok(status);
                }
                thread::sleep(
                    POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
        }

        // The process may have exited between the last poll and this call, in
        // which case `kill` can fail even though it is safe to proceed to wait.
        let _ = self.child.kill();
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }

    #[cfg(test)]
    fn with_termination_grace(mut self, grace: Duration) -> Self {
        self.termination_grace = grace;
        self
    }
}

impl Drop for ProcessSupervisor {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.terminate();
        }
    }
}

#[cfg(unix)]
fn request_graceful_termination(child: &mut Child) -> io::Result<()> {
    let result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn request_graceful_termination(child: &mut Child) -> io::Result<()> {
    // Rust's portable process API only exposes forced termination. Keeping the
    // policy here gives native platform implementations one place to improve.
    child.kill()
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn terminate_allows_a_process_to_handle_the_graceful_signal() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "trap 'exit 42' TERM; while :; do :; done"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = ProcessSupervisor::spawn(&mut command).unwrap();
        thread::sleep(Duration::from_millis(50));

        let status = process.terminate().unwrap();

        assert_eq!(status.code(), Some(42));
    }

    #[cfg(unix)]
    #[test]
    fn terminate_force_kills_a_process_after_the_grace_period() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "trap '' TERM; while :; do :; done"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = ProcessSupervisor::spawn(&mut command)
            .unwrap()
            .with_termination_grace(Duration::from_millis(75));
        thread::sleep(Duration::from_millis(50));
        let started = Instant::now();

        let status = process.terminate().unwrap();

        assert!(!status.success());
        assert!(started.elapsed() >= Duration::from_millis(75));
    }
}
