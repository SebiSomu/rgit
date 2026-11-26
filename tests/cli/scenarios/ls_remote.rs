use insta::assert_snapshot;

use super::three_commits;
use crate::harness::Repo;
use crate::http_server::{GitHttpServer, ServerOptions};

/// Serves a copy of `r`'s repository as `repo.git` on `server`, and masks the server's
/// random address in the transcript.
fn serve(r: &mut Repo, server: &GitHttpServer) {
    let host = server.url("").trim_start_matches("http://").trim_end_matches('/').to_string();
    r.mask(&format!("http://{}", host), "<HTTP>").mask(&host, "<HOST>");
    r.mask(&server.root().to_string_lossy(), "<SRV>");
    server.create_bare_repo("repo.git");
    // `/` on every platform, so the transcript is the same on Windows.
    let bare = format!("{}/repo.git", server.root().to_string_lossy());
    r.run(&format!("push '{}' main", bare));
}

#[test]
fn ls_remote_command_line() {
    let Some(server) = GitHttpServer::start() else {
        eprintln!("skipping: git http-backend is not available");
        return;
    };
    let mut r = three_commits();
    serve(&mut r, &server);
    let url = server.url("repo.git");
    // Without a repository: the current branch's remote, else origin, announced on stderr.
    r.run("ls-remote");
    r.run(&format!("remote add origin {}", url)).run("ls-remote --refs").run("ls-remote -q --heads");
    r.run("ls-remote --get-url").run("ls-remote --get-url some/other");
    r.run(&format!("ls-remote --symref {} HEAD", url));
    r.run(&format!("ls-remote --exit-code {} no-such-ref", url));
    r.run("ls-remote --bogus").run("ls-remote -h");
    r.run(&format!("ls-remote {}", server.url("missing.git")));
    // Fetch and push over HTTPS work with any host (the clone is up to date, so push has nothing to send).
    r.run("fetch origin").run("push origin main");
    assert_snapshot!(r.finish());
}

#[test]
fn ls_remote_asks_the_credential_helper_for_a_login() {
    let options = ServerOptions { basic_auth: Some(("sebi".into(), "s3cret".into())), ..Default::default() };
    let Some(server) = GitHttpServer::start_with(options) else {
        eprintln!("skipping: git http-backend is not available");
        return;
    };
    let mut r = three_commits();
    serve(&mut r, &server);
    let url = server.url("repo.git");
    let host = server.url("").trim_start_matches("http://").trim_end_matches('/').to_string();
    r.set_env("GIT_TERMINAL_PROMPT", "0");
    // No helper and no terminal: git's message.
    r.run(&format!("ls-remote --heads {}", url));
    // A stored login is used, and approved (it stays in the store).
    r.run("config --global credential.helper store");
    r.run_stdin("credential approve", &format!("protocol=http\nhost={}\nusername=sebi\npassword=s3cret\n\n", host));
    r.run(&format!("ls-remote --heads {}", url)).show_file("../home/.git-credentials");
    // A wrong stored login is refused and erased, so the next run would ask again.
    r.run_stdin("credential approve", &format!("protocol=http\nhost={}\nusername=sebi\npassword=wrong\n\n", host));
    r.run(&format!("ls-remote --heads {}", url)).show_file("../home/.git-credentials");
    assert_snapshot!(r.finish());
}
