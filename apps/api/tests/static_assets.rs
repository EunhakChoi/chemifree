//! What the API serves beside its routes when it is given a built web directory: the web files and
//! their cache policy, refusals for every other path, the Host guard and the security headers on
//! every answer, and write protection, which still applies with the web directory attached. Cases
//! without a web directory pin down what the web files replace. The cases over the router use a
//! real directory on disk. The origin and startup cases run the real binary, because its origin
//! policy and its startup checks live in its own startup code.

mod common;

use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use chemifree_api::{ApiState, SESSION_HEADER, WebDir, router};
use common::{Running, TIMEOUT, api_command, run_to_exit};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// The Host and port of the in-process requests: the API's own loopback address.
const PORT: u16 = 3999;
const HOST: &str = "127.0.0.1:3999";
const TOKEN: &str = "static-test-token";

/// The cache policy of a file whose name Vite derives from its content.
const HASHED_POLICY: &str = "public, max-age=31536000, immutable";
/// The cache policy of every other answer, misses included.
const REVALIDATE: &str = "no-cache";
const CSP: &str = "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; \
                   connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";
/// The headers that every answer carries, API and web files alike, with their values.
const SECURITY_HEADERS: [(&str, &str); 4] = [
    ("content-security-policy", CSP),
    ("x-content-type-options", "nosniff"),
    ("referrer-policy", "no-referrer"),
    ("cross-origin-opener-policy", "same-origin"),
];

const INDEX_HTML: &str = "<!doctype html><title>Chemifree fixture</title>";
/// Text that only the fixture's index contains.
const INDEX_MARKER: &str = "Chemifree fixture";
const APP_JS: &str = "console.log('app');";
const APP_CSS: &str = "body { margin: 0; }";
/// Marks the files beside the web directory, which no request may reach.
const OUTSIDE_MARK: &str = "outside-7f3a";
/// Marks the dotfiles that a case writes into the web directory, which no request may read.
const DOTFILE_MARK: &str = "dotfile-5c1d";

/// The API over a fresh data directory, serving a web directory beside its routes.
struct Served {
    router: Router,
    /// Keeps the data directory in place while the router is used.
    _data: TempDir,
}

/// An API over a fresh data directory that serves `web` beside its routes. The API is built with
/// no web origin, so only its own origin may write. The origins that the binary allows when it
/// serves a web directory come from `allowed_origins` in `main.rs`, and these cases do not
/// reproduce them.
fn api_over(web: &Path) -> Served {
    api_with(Some(WebDir::open(web).unwrap()))
}

/// An API over a fresh data directory, with no web directory.
fn api_without_web() -> Served {
    api_with(None)
}

/// An API over a fresh data directory, serving `web` when one is given.
fn api_with(web: Option<WebDir>) -> Served {
    let data = tempfile::tempdir().unwrap();
    let application =
        chemifree_host::local_application(chemifree_host::LocalStores::new(data.path()));
    let state = ApiState::new(application, TOKEN.into(), PORT, &[]).unwrap();
    let state = match web {
        Some(web) => state.with_web_dir(web),
        None => state,
    };
    Served {
        router: router(Arc::new(state)),
        _data: data,
    }
}

/// Writes a web directory as a Vite build leaves it. A secret and a manifest sit beside it, where
/// no request may reach them. Returns the web directory.
fn fixture(root: &Path) -> PathBuf {
    let web = root.join("web");
    fs::create_dir_all(web.join("assets")).unwrap();
    fs::write(web.join("index.html"), INDEX_HTML).unwrap();
    fs::write(web.join("assets/app-3f9c.js"), APP_JS).unwrap();
    fs::write(web.join("assets/style-7d1e.css"), APP_CSS).unwrap();
    fs::write(
        web.join("favicon.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#,
    )
    .unwrap();
    fs::write(root.join("secret"), format!("{OUTSIDE_MARK} secret")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("[package]\nname = \"{OUTSIDE_MARK}\"\n"),
    )
    .unwrap();
    web
}

/// A GET with the API's own Host.
fn get(uri: &str) -> Request<Body> {
    Request::get(uri)
        .header("host", HOST)
        .body(Body::empty())
        .unwrap()
}

/// Sends one request to the API and returns its answer.
async fn ask(api: &Served, request: Request<Body>) -> Response {
    api.router.clone().oneshot(request).await.unwrap()
}

async fn text_of(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn json_of(response: Response) -> Value {
    serde_json::from_str(&text_of(response).await).unwrap()
}

/// The value of a header that the answer must carry.
fn header_of<'a>(response: &'a Response, name: &str) -> &'a str {
    response
        .headers()
        .get(name)
        .unwrap_or_else(|| panic!("the answer carries {name}"))
        .to_str()
        .unwrap()
}

fn assert_security_headers(response: &Response, answer: &str) {
    for (name, value) in SECURITY_HEADERS {
        let actual = response
            .headers()
            .get(name)
            .and_then(|text| text.to_str().ok());
        assert_eq!(actual, Some(value), "{answer} carries {name}");
    }
}

#[tokio::test]
async fn the_index_is_served_at_the_root_and_at_index_html_without_caching() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    for uri in ["/", "/index.html"] {
        let response = ask(&api, get(uri)).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri} is the index");
        assert_eq!(header_of(&response, "content-type"), "text/html", "{uri}");
        assert_eq!(header_of(&response, "cache-control"), REVALIDATE, "{uri}");
        let body = text_of(response).await;
        assert!(body.contains(INDEX_MARKER), "{uri} answered with {body:?}");
    }
}

#[tokio::test]
async fn hashed_assets_are_cached_for_a_year_under_their_own_mime_type() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    for (uri, mime, content) in [
        ("/assets/app-3f9c.js", "text/javascript", APP_JS),
        ("/assets/style-7d1e.css", "text/css", APP_CSS),
    ] {
        let response = ask(&api, get(uri)).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri} is a file");
        assert_eq!(header_of(&response, "content-type"), mime, "{uri}");
        assert_eq!(
            header_of(&response, "cache-control"),
            HASHED_POLICY,
            "{uri}"
        );
        assert_eq!(text_of(response).await, content, "{uri}");
    }
}

#[tokio::test]
async fn a_not_modified_hashed_asset_keeps_its_long_lifetime() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    let first = ask(&api, get("/assets/app-3f9c.js")).await;
    let modified = header_of(&first, "last-modified").to_owned();

    let conditional = Request::get("/assets/app-3f9c.js")
        .header("host", HOST)
        .header("if-modified-since", modified)
        .body(Body::empty())
        .unwrap();
    let response = ask(&api, conditional).await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        header_of(&response, "cache-control"),
        HASHED_POLICY,
        "the revalidated answer keeps the asset's lifetime"
    );
}

#[tokio::test]
async fn other_files_at_the_root_are_served_without_caching() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    let response = ask(&api, get("/favicon.svg")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_of(&response, "content-type"), "image/svg+xml");
    assert_eq!(
        header_of(&response, "cache-control"),
        REVALIDATE,
        "a root file is revalidated"
    );
}

#[tokio::test]
async fn paths_without_a_web_file_are_not_found_and_never_get_the_index() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    for uri in ["/nope", "/documents/42", "/assets/missing-0000.js"] {
        let response = ask(&api, get(uri)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        // A miss is never cached as a file, even under the hashed directory.
        assert_eq!(header_of(&response, "cache-control"), REVALIDATE, "{uri}");
        let body = text_of(response).await;
        assert!(
            !body.contains(INDEX_MARKER),
            "{uri} answered with the index: {body:?}"
        );
    }
}

#[tokio::test]
async fn paths_outside_the_web_directory_are_not_found() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    // Plain names come first. A base directory that reached beside the web directory would serve
    // them, while ServeDir refuses the traversal spellings below whatever the base is.
    for uri in [
        "/secret",
        "/Cargo.toml",
        "/../Cargo.toml",
        "/%2e%2e/Cargo.toml",
        "/..%2fCargo.toml",
        "/%2e%2e/secret",
        "/%2E%2E%2Fsecret",
        "/assets/%2e%2e/%2e%2e/secret",
        "/assets/..%2f..%2fsecret",
        // A NUL byte names no file on any platform, so it is refused like any other miss.
        "/%00",
    ] {
        let response = ask(&api, get(uri)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        let body = text_of(response).await;
        assert!(
            !body.contains(OUTSIDE_MARK),
            "{uri} reached a file beside the web directory: {body:?}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_that_leaves_the_web_directory_is_not_found() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let web = fixture(root.path());
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(
        elsewhere.join("secret.txt"),
        format!("{OUTSIDE_MARK} in a directory"),
    )
    .unwrap();
    symlink(elsewhere, web.join("linked-dir")).unwrap();
    symlink(root.path().join("secret"), web.join("linked-file.txt")).unwrap();
    let api = api_over(&web);

    for uri in ["/linked-dir/secret.txt", "/linked-file.txt"] {
        let response = ask(&api, get(uri)).await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{uri} resolves outside the web directory"
        );
        assert_eq!(header_of(&response, "cache-control"), REVALIDATE, "{uri}");
        let body = text_of(response).await;
        assert!(
            !body.contains(OUTSIDE_MARK),
            "{uri} reached the file beside the web directory: {body:?}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_that_stays_inside_the_web_directory_is_served() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let web = fixture(root.path());
    symlink(web.join("assets/app-3f9c.js"), web.join("alias.js")).unwrap();
    symlink(web.join("assets"), web.join("shortcut")).unwrap();
    let api = api_over(&web);

    for uri in ["/alias.js", "/shortcut/app-3f9c.js"] {
        let response = ask(&api, get(uri)).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{uri} resolves inside the web directory"
        );
        assert_eq!(text_of(response).await, APP_JS, "{uri}");
    }
}

#[tokio::test]
async fn dot_prefixed_web_paths_are_not_found_even_when_the_files_exist() {
    let root = tempfile::tempdir().unwrap();
    let web = fixture(root.path());
    fs::write(web.join(".env"), format!("{DOTFILE_MARK} env")).unwrap();
    fs::create_dir(web.join(".hidden")).unwrap();
    fs::write(web.join(".hidden/x"), format!("{DOTFILE_MARK} hidden")).unwrap();
    fs::create_dir(web.join(".vite")).unwrap();
    fs::write(
        web.join(".vite/manifest.json"),
        format!("{DOTFILE_MARK} manifest"),
    )
    .unwrap();
    let api = api_over(&web);

    // The last spelling escapes the dot, so only a check of the decoded path refuses it.
    for uri in ["/.env", "/.hidden/x", "/.vite/manifest.json", "/%2eenv"] {
        let response = ask(&api, get(uri)).await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{uri} is a dotfile"
        );
        assert_eq!(header_of(&response, "cache-control"), REVALIDATE, "{uri}");
        let body = text_of(response).await;
        assert!(
            !body.contains(DOTFILE_MARK),
            "{uri} reached the dotfile: {body:?}"
        );
    }
}

#[tokio::test]
async fn a_percent_encoded_hashed_asset_keeps_the_hashed_policy() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));

    for uri in ["/%61ssets/app-3f9c.js", "/assets/%61pp-3f9c.js"] {
        let response = ask(&api, get(uri)).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{uri} names the hashed asset"
        );
        assert_eq!(
            header_of(&response, "cache-control"),
            HASHED_POLICY,
            "{uri} is decided by its decoded path"
        );
        assert_eq!(text_of(response).await, APP_JS, "{uri}");
    }
}

#[tokio::test]
async fn unknown_api_paths_keep_the_json_envelope() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    for uri in ["/api", "/api/", "/api/nope", "/api/v1/nope"] {
        let response = ask(&api, get(uri)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(json_of(response).await["code"], "not_found", "{uri}");
    }
    let wrong_method = Request::post("/api/v1/health")
        .header("host", HOST)
        .body(Body::empty())
        .unwrap();
    let response = ask(&api, wrong_method).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_of(response).await["code"], "method_not_allowed");
}

#[tokio::test]
async fn the_api_routes_answer_beside_the_web_app() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    let response = ask(&api, get("/api/v1/health")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_of(response).await["status"], "ok");
}

#[tokio::test]
async fn without_a_web_directory_the_root_is_an_unknown_api_path() {
    let plain = api_without_web();
    let response = ask(&plain, get("/")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_of(response).await["code"], "not_found");
}

#[tokio::test]
async fn a_foreign_host_is_refused_for_web_files_as_for_the_api() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    for uri in ["/", "/assets/app-3f9c.js", "/api/v1/health"] {
        let request = Request::get(uri)
            .header("host", "evil.example")
            .body(Body::empty())
            .unwrap();
        let response = ask(&api, request).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(json_of(response).await["code"], "forbidden_host", "{uri}");
    }
}

#[tokio::test]
async fn every_answer_carries_the_security_headers() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    let plain = api_without_web();
    let foreign_host = Request::get("/")
        .header("host", "evil.example")
        .body(Body::empty())
        .unwrap();
    let answers = [
        ("a web file", ask(&api, get("/")).await),
        ("a missing web file", ask(&api, get("/nope")).await),
        ("an API answer", ask(&api, get("/api/v1/health")).await),
        ("an unknown API path", ask(&api, get("/api/nope")).await),
        ("a refused Host", ask(&api, foreign_host).await),
        (
            "a root without a web directory",
            ask(&plain, get("/")).await,
        ),
    ];
    for (answer, response) in &answers {
        assert_security_headers(response, answer);
    }
}

/// Attaching a web directory does not relax write protection. A write from the API's own origin
/// is admitted, and one from the dev server's origin is refused. The API here is built with no web
/// origin, so this does not test which origins the binary allows when it serves a web directory;
/// `allowed_origins` in `main.rs` and the binary-level case below cover that.
#[tokio::test]
async fn write_protection_still_applies_with_a_web_directory() {
    let root = tempfile::tempdir().unwrap();
    let api = api_over(&fixture(root.path()));
    let post_from = |origin: &str| {
        Request::post("/api/v1/documents")
            .header("host", HOST)
            .header("origin", origin)
            .header("sec-fetch-site", "same-origin")
            .header("content-type", "application/json")
            .header(SESSION_HEADER, TOKEN)
            .body(Body::from(r#"{"request_id":"origin-1"}"#))
            .unwrap()
    };

    let response = ask(&api, post_from("http://127.0.0.1:5173")).await;
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "the dev server's origin is not allowed"
    );
    assert_eq!(json_of(response).await["code"], "forbidden_origin");

    let own = format!("http://127.0.0.1:{PORT}");
    let response = ask(&api, post_from(&own)).await;
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "the API's own origin may write"
    );
}

impl Running {
    /// The origin of the API's own loopback address, which its web app presents.
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The session header name and token that writes present, as `GET /api/v1/session` names them.
    fn session(&self) -> (String, String) {
        let session = exchange(self.port, "GET", "/api/v1/session", &[], "").json();
        let header = session["header"]
            .as_str()
            .expect("the session names its header")
            .to_owned();
        let token = session["token"]
            .as_str()
            .expect("the session carries its token")
            .to_owned();
        (header, token)
    }
}

/// One answer, read to the end of its connection.
struct Reply {
    status: u16,
    body: String,
}

impl Reply {
    fn parse(raw: &[u8]) -> Self {
        let split = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("the answer has a header block");
        let head = String::from_utf8_lossy(&raw[..split]);
        let status_line = head.lines().next().unwrap_or_default();
        let status: u16 = status_line
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("a status code in {status_line:?}"));
        let body = String::from_utf8_lossy(&raw[split + 4..]).into_owned();
        Self { status, body }
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|error| panic!("the body is JSON ({error}): {:?}", self.body))
    }
}

/// Sends one request on a new connection, with the API's own Host, and returns the answer. The
/// body is sent only when it is not empty.
fn exchange(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Reply {
    let mut lines = vec![
        format!("{method} {path} HTTP/1.1"),
        format!("Host: 127.0.0.1:{port}"),
        "Connection: close".to_owned(),
    ];
    lines.extend(
        headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}")),
    );
    if !body.is_empty() {
        lines.push(format!("Content-Length: {}", body.len()));
    }
    let request = format!("{}\r\n\r\n{body}", lines.join("\r\n"));
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the API accepts connections");
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
    Reply::parse(&raw)
}

#[test]
fn the_binary_admits_writes_from_its_own_origin_only_when_it_serves_the_web_app() {
    let root = tempfile::tempdir().unwrap();
    let web = fixture(root.path());
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    let api = Running::start(
        &[
            OsStr::new("--port"),
            OsStr::new("0"),
            OsStr::new("--data-dir"),
            data.as_os_str(),
            OsStr::new("--web-dir"),
            web.as_os_str(),
        ],
        &[],
    );
    let (header, token) = api.session();
    let own = api.origin();

    // The refused write presents everything a valid one does, except that it comes from the dev
    // server's origin. It sends no body: the server answers before reading one, and closing a
    // socket with unread data can reset the connection before the answer is read.
    let refused = exchange(
        api.port,
        "POST",
        "/api/v1/documents",
        &[
            (header.as_str(), token.as_str()),
            ("origin", "http://127.0.0.1:5173"),
            ("sec-fetch-site", "same-origin"),
            ("content-type", "application/json"),
        ],
        "",
    );
    // The origin check runs before the body is read, so a refusal names the origin, not the body.
    assert_eq!(
        refused.json()["code"],
        "forbidden_origin",
        "a write from the dev server's origin is refused for that origin: {}",
        refused.body
    );
    assert_eq!(refused.status, 403, "{}", refused.body);

    let body = json!({"request_id": "own-origin"}).to_string();
    let accepted = exchange(
        api.port,
        "POST",
        "/api/v1/documents",
        &[
            (header.as_str(), token.as_str()),
            ("origin", own.as_str()),
            ("sec-fetch-site", "same-origin"),
            ("content-type", "application/json"),
        ],
        &body,
    );
    assert_eq!(accepted.status, 201, "{}", accepted.body);
}

#[test]
fn the_environment_can_name_the_web_directory() {
    let root = tempfile::tempdir().unwrap();
    let web = fixture(root.path());
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    let api = Running::start(
        &[
            OsStr::new("--port"),
            OsStr::new("0"),
            OsStr::new("--data-dir"),
            data.as_os_str(),
        ],
        &[("CHEMIFREE_WEB_DIR", web.as_os_str())],
    );
    let reply = exchange(api.port, "GET", "/", &[], "");
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(
        reply.body.contains(INDEX_MARKER),
        "the root serves the index: {}",
        reply.body
    );
}

#[test]
fn a_missing_web_directory_stops_startup_before_anything_is_created() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let missing = root.path().join("missing-web");
    // The start would prune this file, if it got past the web directory check.
    stale_temporary(&data);
    let before = listing(root.path());
    let (status, stderr) = run_to_exit(api_command(
        &[
            OsStr::new("--port"),
            OsStr::new("0"),
            OsStr::new("--data-dir"),
            data.as_os_str(),
            OsStr::new("--web-dir"),
            missing.as_os_str(),
        ],
        &[],
    ));

    assert!(
        !status.success(),
        "a missing web directory is a startup error, exit {status:?}: {stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "the error is reported without a panic: {stderr}"
    );
    assert!(stderr.contains("cannot be read"), "{stderr}");
    let shown = missing.display().to_string();
    assert!(
        stderr.contains(&shown),
        "the message names the directory: {stderr}"
    );
    assert_eq!(
        listing(root.path()),
        before,
        "a refused start creates and prunes nothing"
    );
}

/// Writes a temporary file that the store prunes at startup once it is older than an hour: a
/// dot-prefixed `.tmp` file in the documents directory. Returns its path.
fn stale_temporary(data: &Path) -> PathBuf {
    let documents = chemifree_host::LocalStores::new(data)
        .documents_directory()
        .to_path_buf();
    fs::create_dir_all(&documents).unwrap();
    let path = documents.join(".interrupted-write.tmp");
    let file = fs::File::create(&path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(2 * 60 * 60))
        .unwrap();
    path
}

/// Every path under `dir`, sorted, so that two listings of the same tree compare equal.
fn listing(dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path.clone());
            }
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

#[test]
fn a_valid_start_prunes_a_stale_temporary() {
    let root = tempfile::tempdir().unwrap();
    let web = fixture(root.path());
    let data = root.path().join("data");
    let stale = stale_temporary(&data);
    let _api = Running::start(
        &[
            OsStr::new("--port"),
            OsStr::new("0"),
            OsStr::new("--data-dir"),
            data.as_os_str(),
            OsStr::new("--web-dir"),
            web.as_os_str(),
        ],
        &[],
    );
    // The binary prints its documents line after it prunes, so the file is gone by now. The
    // refused start above keeps its file only because a valid start would have removed it.
    assert!(!stale.exists(), "a valid start prunes {}", stale.display());
}

#[test]
fn a_missing_web_directory_is_refused_by_name() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing");
    let error = WebDir::open(&missing).unwrap_err();
    assert!(error.contains("cannot be read"), "{error}");
    assert!(error.contains(&missing.display().to_string()), "{error}");
}

#[test]
fn a_web_directory_without_index_html_is_refused_by_name() {
    let root = tempfile::tempdir().unwrap();
    let bare = root.path().join("bare");
    fs::create_dir(&bare).unwrap();
    let error = WebDir::open(&bare).unwrap_err();
    assert!(error.contains("has no index.html"), "{error}");
    assert!(error.contains(&bare.display().to_string()), "{error}");
}

#[test]
fn a_file_is_refused_as_a_web_directory_by_name() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("index.html");
    fs::write(&file, INDEX_HTML).unwrap();
    let error = WebDir::open(&file).unwrap_err();
    assert!(error.contains("is not a directory"), "{error}");
    assert!(error.contains(&file.display().to_string()), "{error}");
}

#[cfg(unix)]
#[test]
fn the_web_directory_is_kept_by_its_canonical_path() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let web = fixture(root.path());
    // Opened through a link, the directory must still be kept by where it really is.
    let link = root.path().join("web-link");
    symlink(&web, &link).unwrap();
    let opened = WebDir::open(&link).unwrap();
    assert_eq!(opened.path(), web.canonicalize().unwrap());
}

/// Writes a web directory at `root/web` whose `index.html` has mode 0o000. Returns the directory,
/// or `None` after restoring the file when this process reads it anyway, as root does: a refusal
/// can only be observed by a process that cannot read the file.
#[cfg(unix)]
fn web_with_unreadable_index(root: &Path) -> Option<PathBuf> {
    let web = root.join("web");
    fs::create_dir(&web).unwrap();
    fs::write(web.join("index.html"), INDEX_HTML).unwrap();
    set_index_mode(&web, 0o000);
    if fs::File::open(web.join("index.html")).is_ok() {
        set_index_mode(&web, 0o644);
        return None;
    }
    Some(web)
}

/// Sets the mode of the `index.html` in `web`.
#[cfg(unix)]
fn set_index_mode(web: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(web.join("index.html"), fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
#[test]
fn an_unreadable_index_html_is_refused_by_name() {
    let root = tempfile::tempdir().unwrap();
    let Some(web) = web_with_unreadable_index(root.path()) else {
        eprintln!("skipped: this process reads a 0o000 file, as root does");
        return;
    };
    let outcome = WebDir::open(&web);
    // Restored before any assertion, so the temporary directory is cleaned up on every path.
    set_index_mode(&web, 0o644);
    let error = outcome.expect_err("an index.html that cannot be read is refused");
    assert!(error.contains("cannot be read"), "{error}");
    assert!(error.contains(&web.display().to_string()), "{error}");
}

#[cfg(unix)]
#[test]
fn an_unreadable_index_html_stops_startup_before_anything_is_created() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let Some(web) = web_with_unreadable_index(root.path()) else {
        eprintln!("skipped: this process reads a 0o000 file, as root does");
        return;
    };
    // The start would prune this file, if it got past the web directory check.
    stale_temporary(&data);
    let before = listing(root.path());
    let (status, stderr) = run_to_exit(api_command(
        &[
            OsStr::new("--port"),
            OsStr::new("0"),
            OsStr::new("--data-dir"),
            data.as_os_str(),
            OsStr::new("--web-dir"),
            web.as_os_str(),
        ],
        &[],
    ));
    // Restored before any assertion, so the temporary directory is cleaned up on every path.
    set_index_mode(&web, 0o644);

    assert!(
        !status.success(),
        "an unreadable index.html is a startup error, exit {status:?}: {stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "the error is reported without a panic: {stderr}"
    );
    assert!(stderr.contains("cannot be read"), "{stderr}");
    let shown = web.display().to_string();
    assert!(
        stderr.contains(&shown),
        "the message names the directory: {stderr}"
    );
    assert_eq!(
        listing(root.path()),
        before,
        "a refused start creates and prunes nothing"
    );
}
