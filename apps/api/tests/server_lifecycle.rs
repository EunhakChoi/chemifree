#![cfg(unix)]
//! Spawns the real API binary to observe its lifecycle: the startup refusals and their exit codes,
//! the data directory lock that lets one server serve a directory at a time, and the compute
//! concurrency setting. Each case kills and reaps every server it starts before it ends.

mod common;

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{Duration, SystemTime};

use common::{Running, TIMEOUT, api_command, configure_child, run_to_exit};

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

impl Running {
    /// Sends SIGKILL and reaps the server, the way a crash ends it.
    fn sigkill(&mut self) -> ExitStatus {
        self.child.kill().expect("the server can be killed");
        self.child.wait().expect("the killed server can be reaped")
    }
}

/// The API binary with `args`, started by `sh` with a file size limit of zero, so that every write
/// it makes to a file fails with `EFBIG`. The shell ignores SIGXFSZ first, since its default action
/// ends the process, and `exec` keeps an ignored signal ignored.
fn no_file_writes_command(args: &[&str]) -> Command {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(r#"trap "" XFSZ && ulimit -f 0 && exec "$0" "$@""#)
        .arg(env!("CARGO_BIN_EXE_chemifree-api"))
        .args(args);
    configure_child(&mut command, &[]);
    command
}

/// Starts the binary with `args` and `env` and waits for it to exit, which a refused start does at
/// once. Returns its exit code and all of its stderr.
fn refused(args: &[&str], env: &[(&str, &OsStr)]) -> (Option<i32>, String) {
    let (status, stderr) = run_to_exit(api_command(args, env));
    (status.code(), stderr)
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

/// Sets the permission bits of `path`.
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// A data directory, and optionally its lock file, made read-only for the length of a case.
/// Dropping it restores owner-only modes, so the temporary directory can be removed on every path,
/// also a failing one.
struct ReadOnly {
    directory: PathBuf,
    lock_file: Option<PathBuf>,
}

impl ReadOnly {
    /// Makes `directory` read-only, and `lock_file` too when it is given. Returns `None`, after
    /// restoring the modes, when this process can still create files in `directory`, as root can: a
    /// refusal can be observed only where writes are refused.
    fn new(directory: &Path, lock_file: Option<&Path>) -> Option<Self> {
        let read_only = Self {
            directory: directory.to_path_buf(),
            lock_file: lock_file.map(Path::to_path_buf),
        };
        set_mode(&read_only.directory, 0o555).expect("the directory can be made read-only");
        if let Some(lock_file) = &read_only.lock_file {
            set_mode(lock_file, 0o444).expect("the lock file can be made read-only");
        }
        let probe = directory.join("write-probe");
        if OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .is_ok()
        {
            // A probe that could be written means that root made it. It is removed here, and the
            // modes are restored when `read_only` is dropped on return.
            let _ = fs::remove_file(&probe);
            return None;
        }
        Some(read_only)
    }
}

impl Drop for ReadOnly {
    fn drop(&mut self) {
        // A failed restore is ignored here. The cleanup of the temporary directory reports what it
        // cannot remove.
        if let Some(lock_file) = &self.lock_file {
            let _ = set_mode(lock_file, 0o600);
        }
        let _ = set_mode(&self.directory, 0o700);
    }
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
fn a_read_only_data_directory_without_a_lock_file_starts_without_the_lock() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let data_path = directory.path().join("data");
    fs::create_dir(&data_path).expect("the data directory can be created");
    let Some(_read_only) = ReadOnly::new(&data_path, None) else {
        eprintln!("skipped: this process can write to a read-only directory, as root can");
        return;
    };
    let data = path_text(&data_path);
    let lock_path = data_path.join("server.lock");

    let server = Running::start(&["--port", "0", "--data-dir", data], &[]);

    let expected = format!(
        "Server lock not taken: {} cannot be created (Permission denied (os error 13)); the data \
         directory is read-only, so another server may use it at the same time.",
        lock_path.display()
    );
    let warnings: Vec<&str> = server
        .startup
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("Server lock not taken"))
        .collect();
    assert_eq!(
        warnings,
        [expected.as_str()],
        "the start prints one line that says it runs without the lock: {:?}",
        server.startup
    );
    assert!(
        !server
            .startup
            .iter()
            .any(|line| line.starts_with("Could not record")),
        "a server without a lock records nothing: {:?}",
        server.startup
    );
    assert!(
        !lock_path.exists(),
        "no lock file is created in a read-only directory"
    );
    assert_eq!(
        get_status(server.port, "/api/v1/health"),
        200,
        "the server without the lock answers"
    );
}

#[test]
fn a_read_only_lock_file_is_held_through_a_read_only_handle() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let data_path = directory.path().join("data");
    fs::create_dir(&data_path).expect("the data directory can be created");
    let lock_path = data_path.join("server.lock");
    File::create(&lock_path).expect("the lock file can be created");
    // A lock file that this process may write is opened read-write even in a read-only directory,
    // so the lock file is made read-only too.
    let Some(_read_only) = ReadOnly::new(&data_path, Some(&lock_path)) else {
        eprintln!("skipped: this process can write to a read-only directory, as root can");
        return;
    };
    let data = path_text(&data_path);

    let first = Running::start(&["--port", "0", "--data-dir", data], &[]);

    assert_eq!(
        fs::read_to_string(&lock_path).expect("the lock file can be read"),
        "",
        "a server that holds the lock read-only writes no record"
    );
    let refusal = format!(
        "Could not record this server in {}: the lock file was opened read-only",
        lock_path.display()
    );
    assert!(
        first.startup.contains(&refusal),
        "the holder reports that it cannot record its address: {:?}",
        first.startup
    );
    assert_eq!(
        get_status(first.port, "/api/v1/health"),
        200,
        "the server that holds the read-only lock answers"
    );

    let (code, stderr) = refused(&["--port", "0", "--data-dir", data], &[]);

    assert_eq!(
        code,
        Some(EXIT_DATA_DIR_IN_USE),
        "a second server is refused while the read-only lock is held: {stderr}"
    );
    let expected = format!(
        "Chemifree API cannot start: the data directory {data} is already used by another \
         Chemifree API server (pid unknown, address not recorded). Stop that server or pass \
         another --data-dir."
    );
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(
        lines,
        [expected.as_str()],
        "the refusal names no holder, because the read-only holder recorded nothing"
    );
}

#[test]
fn a_record_that_cannot_be_written_neither_stops_the_start_nor_erases_the_old_record() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let data_path = directory.path().join("data");
    fs::create_dir(&data_path).expect("the data directory can be created");
    let lock_path = data_path.join("server.lock");
    let record = "pid=1\nurl=http://127.0.0.1:4242\n";
    fs::write(&lock_path, record).expect("the earlier record can be written");
    let data = path_text(&data_path);

    let server = Running::spawn(no_file_writes_command(&["--port", "0", "--data-dir", data]));

    assert_eq!(
        get_status(server.port, "/api/v1/health"),
        200,
        "the server starts although it cannot write its record"
    );
    assert_eq!(
        fs::read_to_string(&lock_path).expect("the lock file can be read"),
        record,
        "a write that is refused leaves the earlier record in place"
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
        &[("CHEMIFREE_COMPUTE_CONCURRENCY", OsStr::new("0"))],
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
        &[("CHEMIFREE_COMPUTE_CONCURRENCY", OsStr::new("2"))],
    );
    assert_eq!(
        get_status(server.port, "/api/v1/health"),
        200,
        "CHEMIFREE_COMPUTE_CONCURRENCY=2 starts a server that answers"
    );
}
