pub mod cli;
pub mod engine;
pub mod types;

use std::io::Write;
use anyhow::Result;

use crate::remote::http::HttpClient;
use crate::remote::ls_remote::engine::{bundle_refs, format_listing, local_refs, parse_ls_remote_args, select_refs};
use crate::remote::smart_http::discover_refs;
use crate::remote::transport::{resolve_remote, types::Transport};

/// `rgit ls-remote [--heads] [--tags] [--refs] [--symref] [-q] [--exit-code] [--get-url]
/// [<repository> [<patterns>...]]`
///
/// Lists the refs of another repository without fetching anything, in git's exact format.
/// `<repository>` is a configured remote name, a path (a repository or a bundle) or an
/// `http(s)://` URL; it defaults to the current branch's remote, else `origin`. Over HTTPS it
/// speaks git's smart-HTTP protocol (v2, or v0 for older servers) to any host, GitHub
/// included, asking the credential helpers when a server wants a login. Patterns match the end
/// of a ref name (`main` matches `refs/heads/main`). Exit status: 0, 2 with `--exit-code` when
/// nothing matched, 128 for errors, 129 for usage errors.
pub fn ls_remote(args: Vec<String>) -> Result<()> {
    // Like git, every failure is `fatal: <reason>` with status 128.
    if let Err(error) = run(args) {
        let message = error.to_string();
        crate::ui::die_fatal(message.strip_prefix("fatal: ").unwrap_or(&message));
    }
    Ok(())
}

/// The body of `ls-remote`; errors are reported by `ls_remote`.
fn run(args: Vec<String>) -> Result<()> {
    let options = match parse_ls_remote_args(&args) {
        Ok(options) => options,
        Err(message) => crate::ui::die(129, &message),
    };

    let explicit = options.repository.is_some();
    let spec = match &options.repository {
        Some(spec) => spec.clone(),
        None => default_remote().unwrap_or_else(|| crate::ui::die_fatal("No remote configured to list refs from.")),
    };
    let remote = resolve_remote(&spec)?;
    if options.get_url {
        println!("{}", remote.configured_url.as_deref().unwrap_or(&spec));
        return Ok(());
    }

    let mut prefixes = Vec::new();
    if options.tags {
        prefixes.push("refs/tags/".to_string());
    }
    if options.heads {
        prefixes.push("refs/heads/".to_string());
    }
    let refs = match &remote.transport {
        Transport::Local { git_dir } => local_refs(git_dir)?,
        Transport::Bundle { path } => bundle_refs(path)?,
        Transport::SmartHttp { url } => discover_refs(&HttpClient::new()?, url, &prefixes, false)?.refs,
        // ls-remote is read-only, so it uses git's own protocol on GitHub too: one request, and
        // exactly git's answer (peeled tags, symbolic refs).
        Transport::GitHubApi { owner, repo } => {
            let url = crate::remote::transport::github_https_url(remote.configured_url.as_deref().unwrap_or(""), owner, repo);
            discover_refs(&HttpClient::new()?, &url, &prefixes, false)?.refs
        }
    };

    if !explicit && !options.quiet {
        eprintln!("From {}", crate::remote::http::redact_url(&remote.display()));
    }
    let selected = select_refs(&refs, &options);
    let _ = std::io::stdout().lock().write_all(format_listing(&selected, options.symref).as_bytes());
    if options.exit_code && selected.is_empty() {
        crate::ui::exit(2);
    }
    Ok(())
}

/// The remote `ls-remote` uses without an argument: the current branch's
/// `branch.<name>.remote`, else `origin`, as long as that remote has a URL.
fn default_remote() -> Option<String> {
    let from_branch = crate::core::refs::current_branch_ref()
        .ok()
        .and_then(|r| r.strip_prefix("refs/heads/").map(str::to_string))
        .and_then(|b| crate::core::config::get_config_value(&format!("branch.{}.remote", b)).ok().flatten());
    let name = from_branch.unwrap_or_else(|| "origin".to_string());
    crate::core::config::get_config_value(&format!("remote.{}.url", name)).ok().flatten().map(|_| name)
}
