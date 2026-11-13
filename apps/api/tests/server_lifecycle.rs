#![cfg(unix)]
//! Spawns the real API binary to observe its lifecycle: the startup refusals and their exit codes,
//! the data directory lock that lets one server serve a directory at a time, and the compute
//! concurrency setting. Each case kills and reaps every server it starts before it ends.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/// Bounds every wait on a process, so a hung server fails the test instead of stalling it.
const TIMEOUT: Duration = Duration::from_secs(15);
/// Past the one-hour age at which the stores prune an abandoned temporary.
const STALE_AGE: Duration = Duration::from_secs(2 * 60 * 60);
/// The exit status of a startup or serving failure that the cases below do not name.
const EXIT_FAILURE: i32 = 1;
/// The exit status of a command-line usage error, which the argument parser reports.
const EXIT_USAGE: i32 = 2;
/// The exit status of a start whose port is already in use.
const EXIT_PORT_IN_USE: i32 = 3;
/// The exit status of a start whose data directory another live server holds.
const EXIT_DATA_DIR_IN_USE: i32 = 4;
/// The signal that SIGKILL delivers. It ends a process without running its shutdown code.
const SIGKILL: i32 = 9;
/// Environment variables that choose host settings. Each child starts without them, so the
/// developer's shell cannot change what a case observes.
const SELECTION_ENV: [&str; 10] = [
    "CHEMIFREE_COMPUTE_CONCURRENCY",
    "CHEMIFREE_DATA_DIR",
    "CHEMIFREE_INCHI_LIBRARY",
    "CHEMIFREE_INCHI_SHA256",
    "CHEMIFREE_INCHI_TIMEOUT_SECONDS",
    "CHEMIFREE_INCHI_VERSION",
    "CHEMIFREE_LOG_REQUESTS",
    "CHEMIFREE_MODEL_DATA_ROOT",
    "CHEMIFREE_MODEL_TRUSTED_REGISTRY",
    "CHEMIFREE_WEB_DIR",
];

/// A running API server on an ephemeral port. Dropping it kills and reaps the child, also when an
/// assertion fails first.
struct Running {
    child: Child,
    pid: u32,
    port: u16,
}

impl Drop for Running {
    fn drop(&mut self) {
        // The child may have exited already; killing and reaping it is what matters here.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Running {
    /// Starts the binary with `args` and `env`, and returns once it has printed its documents line,
    /// which ends its startup. The port is the one that its listening line names.
    fn start(args: &[&str], env: &[(&str, &str)]) -> Self {
        let mut child = lifecycle_command(args, env)
            .spawn()
            .expect("the API binary starts");
        let lines = stderr_lines(&mut child);
        let mut server = Self {
            pid: child.id(),
            child,
            port: 0,
        };
        // The server is already owned here, so a panic while reading its startup still kills it.
        server.port = listening_port(&lines);
        server
    }

    /// Sends SIGKILL and reaps the server, the way a crash ends it.
    fn sigkill(&mut self) -> ExitStatus {
        self.child.kill().expect("the server can be killed");
        self.child.wait().expect("the killed server can be reaped")
    }
}

/// The API binary with `args` and `env` on top of a cleared selection environment. Its stdin and
/// stdout are closed, and its stderr is piped.
fn lifecycle_command(args: &[&str], env: &[(&str, &str)]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_chemifree-api"));
    for name in SELECTION_ENV {
        command.env_remove(name);
    }
    command
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

/// Reads the child's stderr to its end on a thread, so the child never blocks on a full pipe. The
/// lines arrive on the returned channel, which closes when the stream ends.
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

/// Reads stderr up to the documents line, which the binary prints last before it serves, and
/// returns the port that its listening line names.
fn listening_port(lines: &Receiver<String>) -> u16 {
    let deadline = Instant::now() + TIMEOUT;
    let mut seen: Vec<String> = Vec::new();
    loop {
        let wait = deadline.saturating_duration_since(Instant::now());
        let line = match lines.recv_timeout(wait) {
            Ok(line) => line,
            Err(error) => panic!("no documents line ({error}); stderr so far: {seen:?}"),
        };
        let done = line.starts_with("Documents: ");
        seen.push(line);
        if done {
            break;
        }
    }
    let listening = seen
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
            panic!("the process did not exit within {TIMEOUT:?}: a refused start exits at once");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Starts the binary with `args` and `env` and waits for it to exit, which a refused start does at
/// once. Returns its exit code and all of its stderr.
fn refused(args: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String) {
    let mut child = lifecycle_command(args, env)
        .spawn()
        .expect("the API binary starts");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr
            .read_to_end(&mut bytes)
            .expect("stderr can be read to its end");
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let status = exit_within_timeout(&mut child);
    let text = reader.join().expect("the stderr reader finishes");
    (status.code(), text)
}

/// The status of a GET of `path` on the server at `port`, sent with the server's own Host. The
/// answer is read to the end of the connection, which `Connection: close` ends.
fn get_status(port: u16, path: &str) -> u16 {
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("the server accepts connections");
    stream
        .set_read_timeout(Some(TIMEOUT))
        .expect("a read timeout can be set");
    stream
        .write_all(request.as_bytes())
        .expect("the request is written");
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .expect("the answer is read to the end of the connection");
    let answer = String::from_utf8_lossy(&raw);
    let status_line = answer.lines().next().unwrap_or_default();
    status_line
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("a status code in {status_line:?}"))
}

/// Creates an empty file that looks like an interrupted write from two hours ago.
fn stale_temporary(path: &Path) {
    File::create(path)
        .expect("the temporary file can be created")
        .set_modified(SystemTime::now() - STALE_AGE)
        .expect("its modification time can be set");
}

/// The path as the text that a command-line argument takes. The test paths are UTF-8.
fn path_text(path: &Path) -> &str {
    path.to_str().expect("the test path is UTF-8")
}

#[test]
fn a_port_in_use_is_refused_with_its_own_exit_code() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let data_path = directory.path().join("data");
    let data = path_text(&data_path);
    // Held until the end of the case, so the server's bind meets an address in use.
    let held = TcpListener::bind("127.0.0.1:0").expect("a loopback port can be held");
    let port = held.local_addr().expect("the held address is known").port();
    let port_text = port.to_string();

    let (code, stderr) = refused(&["--port", port_text.as_str(), "--data-dir", data], &[]);

    assert_eq!(
        code,
        Some(EXIT_PORT_IN_USE),
        "a port in use has its own exit code: {stderr}"
    );
    let expected = format!(
        "Chemifree API cannot start: port {port} on 127.0.0.1 is already in use. Stop the program \
         using it, or pass another port with --port <n> (--port 0 picks a free one)."
    );
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(
        lines,
        [expected.as_str()],
        "the refusal is one line that names the port and the way out"
    );
}

#[test]
fn a_data_directory_that_is_a_file_fails_with_the_display_message() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    // A regular file where the data directory should be, so the lock's directory cannot be made.
    let data_path = directory.path().join("data");
    fs::write(&data_path, "not a directory").expect("the blocking file can be written");
    let data = path_text(&data_path);

    let (code, stderr) = refused(&["--port", "0", "--data-dir", data], &[]);

    assert_eq!(
        code,
        Some(EXIT_FAILURE),
        "a server lock that cannot be taken is a failure: {stderr}"
    );
    assert!(
        stderr.contains(&format!("Error: could not take the server lock {data}: ")),
        "the failure prints the lock error's Display message: {stderr}"
    );
    assert!(
        !stderr.contains("Io {") && !stderr.contains("Os {"),
        "the failure is not printed in its Debug form: {stderr}"
    );
}

#[test]
fn a_missing_web_dir_fails_with_its_path_unescaped() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let data_path = directory.path().join("data");
    let data = path_text(&data_path);
    // A double quote is an ordinary file name character on unix, and a Debug report would escape it.
    let web_path = directory.path().join("web \"quoted\"");
    let web = path_text(&web_path);

    let (code, stderr) = refused(&["--port", "0", "--data-dir", data, "--web-dir", web], &[]);

    assert_eq!(
        code,
        Some(EXIT_FAILURE),
        "a web directory that cannot be read is a failure: {stderr}"
    );
    assert!(
        stderr.contains(&format!("Error: web directory {web} cannot be read")),
        "the web directory error names its path verbatim: {stderr}"
    );
}

#[test]
fn port_zero_listens_on_the_port_the_os_picked() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let data_path = directory.path().join("data");
    let data = path_text(&data_path);

    let server = Running::start(&["--port", "0", "--data-dir", data], &[]);

    assert_ne!(
        server.port, 0,
        "the listening line names the port that the OS picked"
    );
    assert_eq!(
        get_status(server.port, "/api/v1/health"),
        200,
        "the server answers on the port that its listening line names"
    );
}

#[test]
fn a_second_server_on_the_same_data_directory_is_refused_naming_the_first() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let data_path = directory.path().join("data");
    let data = path_text(&data_path);
    let first = Running::start(&["--port", "0", "--data-dir", data], &[]);

    // Written after the first server started, so that start did not prune it. A second start that
    // pruned before it checked the lock would remove it.
    let documents = data_path.join("documents");
    fs::create_dir_all(&documents).expect("the documents directory can be created");
    let stale = documents.join(".drawing.4242.0.tmp");
    stale_temporary(&stale);

    let (code, stderr) = refused(&["--port", "0", "--data-dir", data], &[]);

    assert_eq!(
        code,
        Some(EXIT_DATA_DIR_IN_USE),
        "a data directory in use has its own exit code: {stderr}"
    );
    let (pid, port) = (first.pid, first.port);
    let expected = format!(
        "Chemifree API cannot start: the data directory {data} is already used by another \
         Chemifree API server (pid {pid}, http://127.0.0.1:{port}). Stop that server or pass \
         another --data-dir."
    );
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(
        lines,
        [expected.as_str()],
        "the refusal is one line that names the first server"
    );
    assert!(
        stale.exists(),
        "the refused start must leave {} in place",
        stale.display()
    );
    assert_eq!(
        get_status(first.port, "/api/v1/health"),
        200,
        "the first server still answers"
    );
}

#[test]
fn a_killed_server_leaves_no_lock_behind() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let data_path = directory.path().join("data");
    let data = path_text(&data_path);
    let lock_path = data_path.join("server.lock");
    let mut killed = Running::start(&["--port", "0", "--data-dir", data], &[]);
    let status = killed.sigkill();
    assert_eq!(
        status.signal(),
        Some(SIGKILL),
        "the first server died by SIGKILL, so no shutdown code ran: {status:?}"
    );
    // The killed server's record stays behind. The next start takes the lock without removing it.
    let stale_record = fs::read_to_string(&lock_path).expect("the lock record can be read");
    assert_eq!(
        stale_record,
        format!("pid={}\nurl=http://127.0.0.1:{}\n", killed.pid, killed.port),
        "the killed server left its record behind"
    );

    let successor = Running::start(&["--port", "0", "--data-dir", data], &[]);

    assert_eq!(
        get_status(successor.port, "/api/v1/health"),
        200,
        "the successor serves on its own port"
    );
    let record = fs::read_to_string(&lock_path).expect("the lock record can be read");
    assert_eq!(
        record,
        format!(
            "pid={}\nurl=http://127.0.0.1:{}\n",
            successor.pid, successor.port
        ),
        "the successor replaced the record with its own pid and address"
    );
}

#[test]
fn compute_concurrency_outside_1_to_64_is_refused_before_the_data_directory_is_touched() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    // Each case names a data directory that does not exist yet, so a start that got past argument
    // parsing would create it.
    for (index, value) in ["0", "65", "two", "1.5"].into_iter().enumerate() {
        let data_path = root.join(format!("flag-{index}"));
        let data = path_text(&data_path);
        let (code, stderr) = refused(
            &[
                "--port",
                "0",
                "--data-dir",
                data,
                "--compute-concurrency",
                value,
            ],
            &[],
        );
        assert_eq!(
            code,
            Some(EXIT_USAGE),
            "--compute-concurrency {value} is a usage error: {stderr}"
        );
        assert!(
            stderr.contains("compute-concurrency"),
            "the usage error names the flag for {value}: {stderr}"
        );
        assert!(
            !data_path.exists(),
            "a refused start leaves {data} uncreated"
        );
    }

    let data_path = root.join("env");
    let data = path_text(&data_path);
    let (code, stderr) = refused(
        &["--port", "0", "--data-dir", data],
        &[("CHEMIFREE_COMPUTE_CONCURRENCY", "0")],
    );
    assert_eq!(
        code,
        Some(EXIT_USAGE),
        "CHEMIFREE_COMPUTE_CONCURRENCY=0 is a usage error: {stderr}"
    );
    assert!(
        stderr.contains("compute-concurrency"),
        "the usage error names the flag: {stderr}"
    );
    assert!(
        !data_path.exists(),
        "a refused start leaves {data} uncreated"
    );
}

#[test]
fn compute_concurrency_in_range_is_accepted_from_the_flag_and_the_environment() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    for value in ["1", "64"] {
        let data_path = root.join(format!("flag-{value}"));
        let data = path_text(&data_path);
        let server = Running::start(
            &[
                "--port",
                "0",
                "--data-dir",
                data,
                "--compute-concurrency",
                value,
            ],
            &[],
        );
        assert_eq!(
            get_status(server.port, "/api/v1/health"),
            200,
            "--compute-concurrency {value} starts a server that answers"
        );
    }

    let data_path = root.join("env");
    let data = path_text(&data_path);
    let server = Running::start(
        &["--port", "0", "--data-dir", data],
        &[("CHEMIFREE_COMPUTE_CONCURRENCY", "2")],
    );
    assert_eq!(
        get_status(server.port, "/api/v1/health"),
        200,
        "CHEMIFREE_COMPUTE_CONCURRENCY=2 starts a server that answers"
    );
}
