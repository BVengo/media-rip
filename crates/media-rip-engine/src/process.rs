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

/// Capture finite discovery commands while draining both pipes. Cancellation,
/// timeout and read failures all reap the child and join its reader threads.
pub(crate) fn run_cancellable(
    mut command: Command,
    cancellation: &media_rip_core::ports::CancellationToken,
) -> Result<std::process::Output, media_rip_core::MediaEngineError> {
    capture_with_timeout(&mut command, cancellation, Duration::from_secs(120))
}

fn capture_with_timeout(
    command: &mut Command,
    cancellation: &media_rip_core::ports::CancellationToken,
    timeout: Duration,
) -> Result<std::process::Output, media_rip_core::MediaEngineError> {
    use media_rip_core::MediaEngineError;
    use std::process::Stdio;
    if cancellation.is_cancelled() {
        return Err(MediaEngineError::Cancelled);
    }
    let program = command.get_program().to_string_lossy().into_owned();
    let failure = |error: io::Error| MediaEngineError::Process(format!("{program}: {error}"));
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    thread::scope(|scope| {
        let mut process = ProcessSupervisor::spawn(command).map_err(|error| {
            MediaEngineError::Process(format!("could not run {program}: {error}"))
        })?;
        let stdout = process
            .take_stdout()
            .ok_or_else(|| failure(io::Error::other("missing stdout")))?;
        let stderr = process
            .take_stderr()
            .ok_or_else(|| failure(io::Error::other("missing stderr")))?;
        let output_reader = scope.spawn(move || read_bounded(stdout, 16 * 1024 * 1024));
        let error_reader = scope.spawn(move || read_diagnostic_tail(stderr));
        let started = Instant::now();
        loop {
            if cancellation.is_cancelled() {
                return Err(MediaEngineError::Cancelled);
            }
            if started.elapsed() >= timeout {
                return Err(MediaEngineError::Process(format!(
                    "{program} timed out after {} seconds; check the source and retry",
                    timeout.as_secs()
                )));
            }
            if let Some(status) = process.try_wait().map_err(failure)? {
                let stdout = output_reader
                    .join()
                    .map_err(|_| failure(io::Error::other("output reader stopped")))?
                    .map_err(failure)?;
                let stderr = error_reader
                    .join()
                    .map_err(|_| failure(io::Error::other("diagnostic reader stopped")))?
                    .into_bytes();
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            thread::sleep(POLL_INTERVAL);
        }
    })
}

fn read_bounded(mut reader: impl io::Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    let mut overflow = false;
    loop {
        let length = reader.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..length.min(remaining)]);
        overflow |= length > remaining;
    }
    if overflow {
        return Err(io::Error::other("process output exceeded the 16 MiB limit"));
    }
    Ok(output)
}

pub(crate) fn read_diagnostic_tail(mut reader: impl io::Read) -> String {
    use std::collections::VecDeque;
    const LIMIT: usize = 64 * 1024;
    let mut tail = VecDeque::new();
    let mut buffer = [0; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(length) => {
                tail.extend(&buffer[..length]);
                if tail.len() > LIMIT {
                    tail.drain(..tail.len() - LIMIT);
                }
            }
            Err(error) => {
                log::warn!("Could not read process diagnostics: {error}");
                break;
            }
        }
    }
    String::from_utf8_lossy(tail.make_contiguous()).into_owned()
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    use media_rip_core::{MediaEngineError, ports::CancellationToken};
    #[test]
    fn bounded_capture_drains_excess_output_and_reports_failure() {
        assert!(read_bounded(&b"12345678"[..], 4).is_err());
    }
    #[test]
    fn diagnostics_keep_the_end_of_long_output_and_tolerate_invalid_utf8() {
        let mut data = vec![b'x'; 100_000];
        data.extend_from_slice(b"\xfffinal error");
        let tail = read_diagnostic_tail(data.as_slice());
        assert!(tail.ends_with("final error"));
        assert!(tail.len() <= 65538);
    }
    #[cfg(unix)]
    #[test]
    fn timed_out_process_is_terminated() {
        let started = Instant::now();
        let error = capture_with_timeout(
            Command::new("sleep").arg("20"),
            &CancellationToken::default(),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(
            matches!(error, MediaEngineError::Process(message) if message.contains("timed out"))
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
