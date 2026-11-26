//! Unit tests for `src/remote/http.rs`, compiled into that module via `#[path]`. They run the
//! client against the test git server from the CLI tests (real `git http-backend`); each
//! test skips itself when git isn't installed.

use super::*;
use std::sync::Arc;

#[path = "../../cli/http_server.rs"]
mod http_server;
use http_server::{clean_git, GitHttpServer, ServerOptions};

/// A credential source that answers with a fixed login (or none) and records every call.
#[derive(Default)]
struct FakeCredentials {
    answer: Option<(String, String)>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl CredentialSource for FakeCredentials {
    fn fill(&self, credential: &mut Credential) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("fill {}://{}", credential.protocol.as_deref().unwrap_or(""), credential.host.as_deref().unwrap_or("")));
        match &self.answer {
            Some((user, password)) => {
                credential.username = Some(user.clone());
                credential.password = Some(password.clone());
                Ok(())
            }
            None => Err("could not read Username: terminal prompts disabled".to_string()),
        }
    }
    fn approve(&self, credential: &Credential) {
        self.calls.lock().unwrap().push(format!("approve {}", credential.username.as_deref().unwrap_or("")));
    }
    fn reject(&self, credential: &Credential) {
        self.calls.lock().unwrap().push(format!("reject {}", credential.username.as_deref().unwrap_or("")));
    }
}

/// A client with a fake credential source that answers `answer`, and the log of its calls.
fn client(answer: Option<(&str, &str)>) -> (HttpClient, Arc<Mutex<Vec<String>>>) {
    let fake = FakeCredentials { answer: answer.map(|(u, p)| (u.to_string(), p.to_string())), ..Default::default() };
    let calls = fake.calls.clone();
    (HttpClient::with_credentials(Box::new(fake), false).unwrap(), calls)
}

/// A server (with `options`) serving one bare repository with one commit, or `None` without git.
fn server_with_repo(options: ServerOptions) -> Option<(GitHttpServer, tempfile::TempDir)> {
    let server = GitHttpServer::start_with(options)?;
    let bare = server.create_bare_repo("repo.git");
    let work = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| assert!(clean_git().current_dir(work.path()).args(args).status().unwrap().success(), "git {:?}", args);
    run(&["init", "-q", "-b", "main"]);
    std::fs::write(work.path().join("a.txt"), "a\n").unwrap();
    run(&["add", "a.txt"]);
    run(&["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-q", "-m", "one"]);
    run(&["push", "-q", &bare.to_string_lossy(), "main"]);
    Some((server, work))
}

/// The ref advertisement endpoint of the test repository.
fn info_refs(server: &GitHttpServer) -> String {
    format!("{}/info/refs?service=git-upload-pack", server.url("repo.git"))
}

/// Login for the servers that require one.
fn auth() -> ServerOptions {
    ServerOptions { basic_auth: Some(("sebi".into(), "s3cret".into())), ..Default::default() }
}

macro_rules! or_skip {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => {
                eprintln!("skipping: git http-backend is not available");
                return;
            }
        }
    };
}

#[test]
fn get_reaches_a_real_smart_http_server() {
    let (server, _work) = or_skip!(server_with_repo(ServerOptions::default()));
    let (http, calls) = client(None);
    let response = http.get(&info_refs(&server), &[("Git-Protocol", "version=2")]).unwrap().check_status(&server.url("repo.git")).unwrap();
    assert_eq!(response.content_type().as_deref(), Some("application/x-git-upload-pack-advertisement"));
    let body = response.bytes().unwrap();
    // Protocol v2 answers with its version line first.
    assert!(String::from_utf8_lossy(&body).contains("version 2"), "{:?}", String::from_utf8_lossy(&body));
    let request = &server.requests()[0];
    assert!(request.headers["user-agent"].starts_with("git/"), "{:?}", request.headers);
    assert_eq!(request.headers.get("git-protocol").map(String::as_str), Some("version=2"));
    assert!(!request.headers.contains_key("authorization"));
    assert!(calls.lock().unwrap().is_empty(), "no login needed, none asked for");
}

#[test]
fn a_401_asks_for_a_login_retries_and_approves_it_once() {
    let (server, _work) = or_skip!(server_with_repo(auth()));
    let (http, calls) = client(Some(("sebi", "s3cret")));
    let response = http.get(&info_refs(&server), &[]).unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let host = server.url("").trim_start_matches("http://").trim_end_matches('/').to_string();
    assert_eq!(*calls.lock().unwrap(), vec![format!("fill http://{}", host), "approve sebi".to_string()]);

    // The next request sends the login straight away, without asking again.
    http.get(&info_refs(&server), &[]).unwrap();
    assert_eq!(calls.lock().unwrap().len(), 2);
    let statuses: Vec<u16> = server.requests().iter().map(|r| r.status).collect();
    assert_eq!(statuses, vec![401, 200, 200]);
    assert!(!server.requests()[0].headers.contains_key("authorization"), "the first try goes without a login");
    assert!(server.requests()[2].headers.contains_key("authorization"));
}

#[test]
fn a_refused_login_is_rejected_and_the_request_fails() {
    let (server, _work) = or_skip!(server_with_repo(auth()));
    let (http, calls) = client(Some(("sebi", "wrong")));
    let err = http.get(&info_refs(&server), &[]).err().unwrap().to_string();
    assert_eq!(err, format!("fatal: Authentication failed for '{}'", info_refs(&server)));
    let calls = calls.lock().unwrap();
    assert!(calls[0].starts_with("fill "));
    assert_eq!(calls[1..], ["reject sebi".to_string()]);
    assert!(!err.contains("wrong"), "the password never appears in messages");
}

#[test]
fn no_login_available_fails_like_git() {
    let (server, _work) = or_skip!(server_with_repo(auth()));
    let (http, _) = client(None);
    let err = http.get(&info_refs(&server), &[]).err().unwrap().to_string();
    assert_eq!(err, "fatal: could not read Username: terminal prompts disabled");
}

#[test]
fn a_login_in_the_url_is_used_as_given() {
    let (server, _work) = or_skip!(server_with_repo(auth()));
    let (http, calls) = client(None);
    let url = format!("{}/info/refs?service=git-upload-pack", server.url_with_login("repo.git", "sebi", "s3cret"));
    assert_eq!(http.get(&url, &[]).unwrap().status(), StatusCode::OK);
    assert_eq!(*calls.lock().unwrap(), vec!["approve sebi".to_string()], "nothing to fill");
    assert_eq!(server.requests().len(), 1, "sent with the first request");
}

#[test]
fn missing_repositories_and_errors_read_like_git() {
    let (server, _work) = or_skip!(server_with_repo(ServerOptions::default()));
    let (http, _) = client(None);
    let missing = server.url("nope.git");
    let response = http.get(&format!("{}/info/refs?service=git-upload-pack", missing), &[]).unwrap();
    assert_eq!(response.check_status(&missing).err().unwrap().to_string(), format!("fatal: repository '{}' not found", missing));
}

#[test]
fn passwords_are_never_sent_over_plain_http_to_other_machines() {
    let (http, calls) = client(None);
    // Refused before anything is sent: example.invalid is never even resolved.
    let err = http.get("http://sebi:s3cret@example.invalid/repo.git/info/refs", &[]).err().unwrap().to_string();
    assert!(err.starts_with("fatal: refusing to send a password over unencrypted http to 'example.invalid'"), "{}", err);
    assert!(!err.contains("s3cret"));
    assert!(calls.lock().unwrap().is_empty());
    let (allowed, _) = (HttpClient::with_credentials(Box::new(FakeCredentials::default()), true).unwrap(), ());
    let err = allowed.get("http://sebi:s3cret@127.0.0.1:1/repo.git/info/refs", &[]).err().unwrap().to_string();
    assert!(err.starts_with("fatal: unable to access 'http://127.0.0.1:1/repo.git/info/refs'"), "{}", err);
}

#[test]
fn a_login_does_not_follow_a_redirect_to_another_host() {
    let (target, _work) = or_skip!(server_with_repo(ServerOptions::default()));
    let redirector = or_skip!(GitHttpServer::start_with(ServerOptions { redirect_to: Some(target.url("")), ..Default::default() }));
    let (http, _) = client(None);
    let url = format!("{}/info/refs?service=git-upload-pack", redirector.url_with_login("repo.git", "sebi", "s3cret"));
    let response = http.get(&url, &[]).unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.final_url().as_str(), info_refs(&target), "the new base URL is visible to callers");
    assert!(redirector.requests()[0].headers.contains_key("authorization"));
    assert!(!target.requests()[0].headers.contains_key("authorization"), "the other host never sees the login");
}

#[test]
fn redact_url_drops_logins() {
    assert_eq!(redact_url("https://me:tok@github.com/o/r.git"), "https://github.com/o/r.git");
    assert_eq!(redact_url("https://me@github.com/o/r"), "https://github.com/o/r");
    assert_eq!(redact_url("weird://me:tok@host"), "weird://host");
    assert_eq!(redact_url("not a url"), "not a url");
}
