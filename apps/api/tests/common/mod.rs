//! Helpers for the test binaries that run the real API process. Each binary includes this module
//! with `mod common;`, so an item that a binary does not use is marked `dead_code` with its reason.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// Bounds every wait on the API process, so a hung process fails the test instead of stalling it.
pub const TIMEOUT: Duration = Duration::from_secs(15);

/// The environment variables that the API process reads. A child starts without them, so a
/// developer's shell cannot change what a case observes.
pub const SELECTION_ENV: [&str; 11] = [
    "CHEMIFREE_COMPUTE_CONCURRENCY",
    "CHEMIFREE_DATA_DIR",
    "CHEMIFREE_INCHI_LIBRARY",
    "CHEMIFREE_INCHI_SHA256",
    "CHEMIFREE_INCHI_TIMEOUT_SECONDS",
    "CHEMIFREE_INCHI_VERSION",
    "CHEMIFREE_LOG_REQUESTS",
    "CHEMIFREE_MODEL_DATA_ROOT",
    "CHEMIFREE_MODEL_TRUSTED_REGISTRY",
    // The file store pauses each document save and trash by this many milliseconds. It does so in
    // debug builds, which `cargo test` uses, so the API process reads this variable too.
    "CHEMIFREE_TEST_COMMIT_DELAY_MS",
    "CHEMIFREE_WEB_DIR",
];

/// The API binary with `args` and `env`, configured as [`configure_child`] describes.
pub fn api_command(args: &[impl AsRef<OsStr>], env: &[(&str, &OsStr)]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_chemifree-api"));
    command.args(args);
    configure_child(&mut command, env);
    command
}

/// Applies the setup that every child process shares. The selection environment is cleared first,
/// then `env` is set, so that `env` can name a selection variable. Stdin and stdout are closed,
/// and stderr is piped.
pub fn configure_child(command: &mut Command, env: &[(&str, &OsStr)]) {
    for name in SELECTION_ENV {
        command.env_remove(name);
    }
    command
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
}

/// An API process that has printed its documents line, which ends its startup. Dropping it kills
/// and reaps the child, also when an assertion fails first.
pub struct Running {
    pub child: Child,
    /// The process id.
    #[allow(dead_code, reason = "only server_lifecycle reads the process id")]
    pub pid: u32,
    /// The port that the listening line names.
    pub port: u16,
    /// Every stderr line up to and including the documents line, in order.
    #[allow(dead_code, reason = "static_assets never reads the startup lines")]
    pub startup: Vec<String>,
}

impl Drop for Running {
    fn drop(&mut self) {
        // The child may have exited already; killing and reaping it is what matters here.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Running {
    /// Starts the API binary with `args` and `env`, and returns once it has printed its documents
    /// line, which ends its startup.
    pub fn start(args: &[impl AsRef<OsStr>], env: &[(&str, &OsStr)]) -> Self {
        Self::spawn(api_command(args, env))
    }

    /// Starts `command`, which runs the API binary, and waits for its startup as [`Running::start`]
    /// does.
    pub fn spawn(mut command: Command) -> Self {
        let mut child = command.spawn().expect("the API binary starts");
        let lines = stderr_lines(&mut child);
        let mut api = Self {
            pid: child.id(),
            child,
            port: 0,
            startup: Vec::new(),
        };
        // The child is already owned here, so a panic while reading its startup still kills it.
        api.startup = startup_lines(&lines);
        api.port = listening_port(&api.startup);
        api
    }
}

/// Reads stderr on a thread, so that the child never blocks on a full pipe. The lines arrive on the
/// returned channel, which closes when the stream ends.
fn stderr_lines(child: &mut Child) -> Receiver<String> {
    let stderr = child.stderr.take().expect("stderr is piped");
    let (sender, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            // The receiver may be gone by now. The pipe is still read to its end, so the child
            // never blocks on it.
            let _ = sender.send(line);
        }
    });
    lines
}

/// Reads stderr up to the documents line, which the API prints last before it serves, and returns
/// every line read, that line included.
fn startup_lines(lines: &Receiver<String>) -> Vec<String> {
    let deadline = Instant::now() + TIMEOUT;
    let mut seen: Vec<String> = Vec::new();
    loop {
        let wait = deadline.saturating_duration_since(Instant::now());
        let line = match lines.recv_timeout(wait) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => {
                panic!("no documents line within {TIMEOUT:?}; stderr so far: {seen:?}")
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("the API exited before its documents line; stderr so far: {seen:?}")
            }
        };
        let done = line.starts_with("Documents: ");
        seen.push(line);
        if done {
            return seen;
        }
    }
}

/// The port that the listening line of `startup` names.
fn listening_port(startup: &[String]) -> u16 {
    let listening = startup
        .iter()
        .find_map(|line| line.strip_prefix("Chemifree API listening on http://127.0.0.1:"))
        .expect("the listening line names the loopback address");
    listening
        .parse()
        .expect("the listening line ends with a port number")
}

/// Waits for the child to exit. Kills and reaps it, then fails the test, when it has not exited in
/// time.
fn exit_within_timeout(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().expect("the child can be polled") {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the API did not exit within {TIMEOUT:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Runs `command` to its exit, which a refused start reaches at once, and returns its status and
/// all of its stderr. Kills and reaps the child when it does not exit in time.
#[allow(dead_code, reason = "startup_assembly never waits for an exit")]
pub fn run_to_exit(mut command: Command) -> (ExitStatus, String) {
    let mut child = command.spawn().expect("the API binary starts");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    // The pipe is read on a thread while the child runs, so a child that writes a lot never blocks.
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr
            .read_to_end(&mut bytes)
            .expect("stderr can be read to its end");
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let status = exit_within_timeout(&mut child);
    let text = reader.join().expect("the stderr reader finishes");
    (status, text)
}
