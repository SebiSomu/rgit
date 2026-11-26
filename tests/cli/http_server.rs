//! A git server over HTTP for tests: a small HTTP/1.1 server on 127.0.0.1 that runs
//! `git http-backend` (git's own smart-HTTP CGI program) for every request, serving bare
//! repositories from a temporary directory. Lets the HTTPS transport be tested against a real
//! git server with no network and no accounts. Needs `git`; `start` returns `None` without it,
//! and tests then skip themselves.
//!
//! Options: HTTP Basic authentication (to test the credential loop), and a log of every
//! request (method, path, headers, status) so tests can check what a client sent.

#![allow(dead_code)] // Used by the HTTPS tests from step 3 on; the self-tests below use most of it.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use base64::Engine;

/// Environment variables that point git or HTTP clients somewhere else; children never inherit them.
const PROXY_VARS: &[&str] = &["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"];

/// One request as the server saw it.
#[derive(Debug, Clone)]
pub struct RequestRecord {
    pub method: String,
    /// The path with its query string, e.g. `/repo.git/info/refs?service=git-upload-pack`.
    pub target: String,
    /// Header names in lower case.
    pub headers: HashMap<String, String>,
    pub status: u16,
}

/// How the server behaves.
#[derive(Debug, Clone, Default)]
pub struct ServerOptions {
    /// Require HTTP Basic authentication with this user and password on every request.
    pub basic_auth: Option<(String, String)>,
}

pub struct GitHttpServer {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    log: Arc<Mutex<Vec<RequestRecord>>>,
    thread: Option<JoinHandle<()>>,
}

impl GitHttpServer {
    /// Starts a server without authentication, or `None` when `git http-backend` can't run.
    pub fn start() -> Option<GitHttpServer> {
        GitHttpServer::start_with(ServerOptions::default())
    }

    /// Starts a server with `options`, or `None` when `git http-backend` can't run.
    pub fn start_with(options: ServerOptions) -> Option<GitHttpServer> {
        let tmp = tempfile::tempdir().ok()?;
        let root = tmp.path().canonicalize().ok()?.join("srv");
        std::fs::create_dir_all(&root).ok()?;
        if !backend_available(&root) {
            return None;
        }
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let addr = listener.local_addr().ok()?;
        let stop = Arc::new(AtomicBool::new(false));
        let log = Arc::new(Mutex::new(Vec::new()));
        let home = tmp.path().canonicalize().ok()?.join("server-home");
        std::fs::create_dir_all(&home).ok()?;

        let (thread_stop, thread_log, thread_root) = (stop.clone(), log.clone(), root.clone());
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (log, root, home, options) = (thread_log.clone(), thread_root.clone(), home.clone(), options.clone());
                std::thread::spawn(move || {
                    let _ = serve_connection(stream, &root, &home, &options, &log);
                });
            }
        });
        Some(GitHttpServer { _tmp: tmp, root, addr, stop, log, thread: Some(thread) })
    }

    /// The directory the repositories are served from (`GIT_PROJECT_ROOT`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The URL of repository `name` (a path under `root`), e.g. `http://127.0.0.1:41234/repo.git`.
    pub fn url(&self, name: &str) -> String {
        format!("http://{}/{}", self.addr, name)
    }

    /// The same URL with a user and password in it, for clients that take credentials that way.
    pub fn url_with_login(&self, name: &str, user: &str, password: &str) -> String {
        format!("http://{}:{}@{}/{}", user, password, self.addr, name)
    }

    /// Every request served so far, oldest first.
    pub fn requests(&self) -> Vec<RequestRecord> {
        self.log.lock().unwrap().clone()
    }

    /// Creates an empty bare repository `name` under `root` that accepts pushes.
    pub fn create_bare_repo(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        let status = clean_git().args(["init", "--bare", "-q", "-b", "main"]).arg(&path).status().expect("run git init --bare");
        assert!(status.success(), "git init --bare failed");
        let status = clean_git().arg("-C").arg(&path).args(["config", "http.receivepack", "true"]).status().expect("run git config");
        assert!(status.success(), "git config http.receivepack failed");
        path
    }
}

impl Drop for GitHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // `incoming()` blocks: one connection wakes it so the loop sees `stop`.
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A `git` command that ignores the developer's git config and environment and never goes
/// through an HTTP proxy, so it behaves the same on every machine.
pub fn clean_git() -> Command {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(&key);
        }
    }
    for var in PROXY_VARS {
        command.env_remove(var);
    }
    command.env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", null_device()).env("GIT_TERMINAL_PROMPT", "0").env("NO_PROXY", "*").env("no_proxy", "*");
    command
}

/// The file that is always empty (`/dev/null`, `NUL` on Windows), used as an empty global config.
fn null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

/// Whether `git http-backend` runs here: it answers a request for a missing repository with
/// a CGI `Status:` header.
fn backend_available(root: &Path) -> bool {
    let output = clean_git()
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", root)
        .env("REQUEST_METHOD", "GET")
        .env("PATH_INFO", "/missing.git/info/refs")
        .env("QUERY_STRING", "service=git-upload-pack")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    matches!(output, Ok(out) if String::from_utf8_lossy(&out.stdout).contains("Status:"))
}

/// Serves one request on `stream` (the server always closes the connection afterwards).
fn serve_connection(stream: TcpStream, root: &Path, home: &Path, options: &ServerOptions, log: &Mutex<Vec<RequestRecord>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let body = read_body(&mut reader, &headers)?;

    let mut record = RequestRecord { method: method.clone(), target: target.clone(), headers: headers.clone(), status: 0 };
    let user = match check_auth(options, &headers) {
        Ok(user) => user,
        Err(()) => {
            record.status = 401;
            log.lock().unwrap().push(record);
            let response = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"rgit-test\"\r\nContent-Type: text/plain\r\nContent-Length: 14\r\nConnection: close\r\n\r\nauth required\n";
            return writer.write_all(response.as_bytes());
        }
    };

    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let mut command = clean_git();
    command
        .arg("http-backend")
        .env("HOME", home)
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("REQUEST_METHOD", &method)
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("REMOTE_ADDR", "127.0.0.1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (header, variable) in [("content-type", "CONTENT_TYPE"), ("git-protocol", "GIT_PROTOCOL"), ("content-encoding", "HTTP_CONTENT_ENCODING")] {
        if let Some(value) = headers.get(header) {
            command.env(variable, value);
        }
    }
    if let Some(user) = user {
        command.env("REMOTE_USER", user);
    }
    let mut child = command.spawn()?;
    // Feed the body from another thread: a large push could otherwise fill both pipes.
    let mut stdin = child.stdin.take().expect("piped stdin");
    let feeder = std::thread::spawn(move || {
        let _ = stdin.write_all(&body);
    });
    let output = child.wait_with_output()?;
    let _ = feeder.join();

    let (status_line, cgi_headers, cgi_body) = split_cgi_output(&output.stdout);
    record.status = status_line.split_whitespace().next().and_then(|code| code.parse().ok()).unwrap_or(500);
    log.lock().unwrap().push(record);

    let mut response = format!("HTTP/1.1 {}\r\n", status_line);
    for header in cgi_headers {
        response.push_str(&header);
        response.push_str("\r\n");
    }
    response.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", cgi_body.len()));
    writer.write_all(response.as_bytes())?;
    writer.write_all(cgi_body)?;
    writer.flush()
}

/// The request body, from `Content-Length` or a chunked transfer (git streams large pushes).
fn read_body<R: BufRead>(reader: &mut R, headers: &HashMap<String, String>) -> std::io::Result<Vec<u8>> {
    if headers.get("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        let mut body = Vec::new();
        loop {
            let mut size_line = String::new();
            reader.read_line(&mut size_line)?;
            let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("0"), 16).unwrap_or(0);
            if size == 0 {
                // The trailer section ends with an empty line.
                let mut line = String::new();
                while reader.read_line(&mut line)? > 0 && !line.trim().is_empty() {
                    line.clear();
                }
                return Ok(body);
            }
            let mut chunk = vec![0u8; size];
            reader.read_exact(&mut chunk)?;
            body.extend_from_slice(&chunk);
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf)?;
        }
    }
    let length: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(body)
}

/// `Ok(Some(user))` for valid credentials, `Ok(None)` when none are required, `Err` otherwise.
fn check_auth(options: &ServerOptions, headers: &HashMap<String, String>) -> Result<Option<String>, ()> {
    let Some((user, password)) = &options.basic_auth else { return Ok(None) };
    let expected = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", user, password)));
    match headers.get("authorization") {
        Some(given) if *given == expected => Ok(Some(user.clone())),
        _ => Err(()),
    }
}

/// Splits CGI output into the HTTP status (`200 OK` unless a `Status:` header says
/// otherwise), the other headers, and the body.
fn split_cgi_output(output: &[u8]) -> (String, Vec<String>, &[u8]) {
    let (head_end, body_start) = match find(output, b"\r\n\r\n") {
        Some(i) => (i, i + 4),
        None => match find(output, b"\n\n") {
            Some(i) => (i, i + 2),
            None => (output.len(), output.len()),
        },
    };
    let head = String::from_utf8_lossy(&output[..head_end]);
    let mut status = "200 OK".to_string();
    let mut headers = Vec::new();
    for line in head.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty()) {
        match line.split_once(':') {
            Some((name, value)) if name.eq_ignore_ascii_case("status") => status = value.trim().to_string(),
            _ => headers.push(line.to_string()),
        }
    }
    (status, headers, &output[body_start..])
}

/// The first position of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Self-tests: real `git` as the client proves the server speaks smart HTTP correctly, before
/// any rgit code relies on it.
#[cfg(test)]
mod tests {
    use super::*;

    /// A work tree with one commit, made with the clean `git`.
    fn work_repo(dir: &Path) {
        let run = |args: &[&str]| {
            let status = clean_git().current_dir(dir).args(args).status().expect("run git");
            assert!(status.success(), "git {:?} failed", args);
        };
        std::fs::create_dir_all(dir).unwrap();
        run(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["-c", "user.name=T", "-c", "user.email=t@example.com", "commit", "-q", "-m", "first"]);
    }

    /// Runs the clean `git` and returns (success, stderr).
    fn git_in(dir: &Path, args: &[&str]) -> (bool, String) {
        let output = clean_git().current_dir(dir).args(args).output().expect("run git");
        (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
    }

    #[test]
    fn real_git_pushes_and_clones_over_http() {
        let Some(server) = GitHttpServer::start() else {
            eprintln!("skipping: git http-backend is not available");
            return;
        };
        server.create_bare_repo("repo.git");
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        work_repo(&work);

        let (ok, err) = git_in(&work, &["push", &server.url("repo.git"), "main"]);
        assert!(ok, "push over HTTP failed: {}", err);

        let (ok, err) = git_in(tmp.path(), &["clone", "-q", &server.url("repo.git"), "copy"]);
        assert!(ok, "clone over HTTP failed: {}", err);
        assert_eq!(std::fs::read_to_string(tmp.path().join("copy/a.txt")).unwrap(), "hello\n");

        let requests = server.requests();
        let targets: Vec<&str> = requests.iter().map(|r| r.target.as_str()).collect();
        assert!(targets.contains(&"/repo.git/info/refs?service=git-receive-pack"), "{:?}", targets);
        assert!(targets.contains(&"/repo.git/git-receive-pack"), "{:?}", targets);
        assert!(targets.contains(&"/repo.git/git-upload-pack"), "{:?}", targets);
        assert!(requests.iter().all(|r| r.status == 200), "{:?}", requests);
        // git asks for protocol v2 when fetching; the server passes that on to http-backend.
        let discovery = requests.iter().find(|r| r.target == "/repo.git/info/refs?service=git-upload-pack").unwrap();
        assert_eq!(discovery.headers.get("git-protocol").map(String::as_str), Some("version=2"));
    }

    #[test]
    fn basic_auth_is_required_when_configured() {
        let options = ServerOptions { basic_auth: Some(("sebi".into(), "s3cret".into())) };
        let Some(server) = GitHttpServer::start_with(options) else {
            eprintln!("skipping: git http-backend is not available");
            return;
        };
        let bare = server.create_bare_repo("private.git");
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        work_repo(&work);
        let (ok, err) = git_in(&work, &["push", "-q", &bare.to_string_lossy(), "main"]);
        assert!(ok, "seeding the bare repository failed: {}", err);

        let (ok, err) = git_in(tmp.path(), &["clone", "-q", &server.url("private.git"), "denied"]);
        assert!(!ok, "clone without credentials should fail");
        assert!(err.contains("could not read Username") || err.contains("Authentication failed"), "{}", err);
        assert_eq!(server.requests().last().map(|r| r.status), Some(401));

        let (ok, err) = git_in(tmp.path(), &["clone", "-q", &server.url_with_login("private.git", "sebi", "s3cret"), "allowed"]);
        assert!(ok, "clone with credentials failed: {}", err);
        assert!(server.requests().iter().any(|r| r.status == 200 && r.headers.contains_key("authorization")));

        let (ok, _) = git_in(tmp.path(), &["clone", "-q", &server.url_with_login("private.git", "sebi", "wrong"), "wrong"]);
        assert!(!ok, "a wrong password must be refused");
    }

    #[test]
    fn missing_repositories_are_404() {
        let Some(server) = GitHttpServer::start() else {
            eprintln!("skipping: git http-backend is not available");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let (ok, err) = git_in(tmp.path(), &["ls-remote", &server.url("nope.git")]);
        assert!(!ok);
        assert!(err.contains("not found") || err.contains("404"), "{}", err);
        assert_eq!(server.requests().last().map(|r| r.status), Some(404));
    }
}
