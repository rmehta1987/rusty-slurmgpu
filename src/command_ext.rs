use std::io;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Default timeout for fast Slurm commands like scontrol/squeue (30 seconds).
pub(crate) const SLURM_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for sacct queries, which can be slow over large time ranges (5 minutes).
pub(crate) const SACCT_TIMEOUT: Duration = Duration::from_secs(300);

/// Run a command with a timeout, returning an error if it takes too long.
///
/// Spawns the command in a background thread and waits with a timeout.
/// If the timeout expires, the process is killed and an error is returned.
pub(crate) fn run_with_timeout(mut cmd: Command, timeout: Duration) -> io::Result<Output> {
    let child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

    let pid = child.id();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Kill the timed-out process
            let _ = Command::new("kill").arg(pid.to_string()).output();
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("Slurm command timed out after {}s", timeout.as_secs()),
            ))
        }
        Err(_) => Err(io::Error::other("Command thread panicked")),
    }
}
