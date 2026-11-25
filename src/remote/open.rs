pub mod cli;
pub mod engine;
pub mod types;

use crate::core::refs;
use crate::remote::open::engine::{launch, split_lines, url_for, web_repo};
use crate::remote::open::types::Target;

/// `rgit open [<target>] [-p] [--remote <name>]`: prints the web URL of the remote's page for
/// a file or directory (`path:12-20` for lines), a branch, a commit, a new pull request from
/// the current branch (`pr`) or an existing one (`#123`), and opens it in the browser (`-p`
/// only prints). Default: the current branch. Knows GitHub, GitLab and Bitbucket URLs. Files
/// are shown at the current branch when the remote has it, else at the commit HEAD is on.
pub fn open(target: Option<String>, print_only: bool, remote: Option<String>) -> anyhow::Result<()> {
    let remotes = crate::remote::remote::engine::list_remote_names()?;
    let remote = match remote {
        Some(r) if remotes.contains(&r) => r,
        Some(r) => crate::ui::die_fatal(format!("no remote named '{}'", r)),
        None if remotes.iter().any(|r| r == "origin") => "origin".to_string(),
        None if remotes.len() == 1 => remotes[0].clone(),
        None if remotes.is_empty() => crate::ui::die_fatal("this repository has no remote, so it has no web page\nhint: `rgit remote add origin <url>`"),
        None => crate::ui::die_fatal(format!("several remotes ({}): pick one with --remote <name>", remotes.join(", "))),
    };
    let url = crate::remote::remote::engine::get_remote_urls(&remote)?.0.unwrap_or_default();
    let Some(repo) = web_repo(&url) else {
        crate::ui::die_fatal(format!("can't tell the web page of {} ({}): rgit knows GitHub, GitLab and Bitbucket URLs", remote, url));
    };

    let head = refs::resolve_head_commit()?;
    let branch = match refs::resolve_head()? {
        refs::HeadState::Branch(b) => Some(b),
        refs::HeadState::Detached(_) => None,
    };
    let on_remote = |b: &str| refs::read_ref(&format!("refs/remotes/{}/{}", remote, b)).ok().flatten().is_some();
    // Files are shown at the current branch if the remote has it, otherwise at HEAD's commit.
    let here = || -> String {
        match &branch {
            Some(b) if on_remote(b) => b.clone(),
            _ => head.clone().unwrap_or_else(|| crate::ui::die_fatal("there are no commits yet")),
        }
    };

    let chosen = match target.as_deref() {
        None => match &branch {
            Some(b) if on_remote(b) => Target::Tree { reference: b.clone() },
            Some(b) => crate::ui::die_fatal(format!("'{}' isn't on {} yet, so it has no page there\nhint: `rgit push -u {} {}` first", b, remote, remote, b)),
            None => Target::Commit(head.clone().unwrap_or_else(|| crate::ui::die_fatal("there are no commits yet"))),
        },
        Some("pr") => {
            let Some(b) = &branch else { crate::ui::die_fatal("a pull request needs a branch: HEAD is detached") };
            if !on_remote(b) {
                crate::ui::die_fatal(format!("push '{}' first (`rgit push -u {} {}`): a pull request is made from the remote's branch", b, remote, b));
            }
            let base = crate::remote::pr_summary::engine::default_base(&remote).map(|(name, _)| name.trim_start_matches(&format!("{}/", remote)).to_string()).unwrap_or_else(|| "main".to_string());
            Target::NewPullRequest { branch: b.clone(), base }
        }
        Some(t) if t.starts_with('#') && t[1..].parse::<u64>().is_ok() => Target::PullRequest(t[1..].parse().unwrap()),
        Some(t) => {
            let (path, lines) = split_lines(t);
            let clean = path.trim_start_matches("./").trim_end_matches('/').replace('\\', "/");
            let reference = here();
            let tree_of = head.as_deref().map(|h| {
                let mut tree = std::collections::BTreeMap::new();
                let _ = crate::core::object_store::flatten_tree(&crate::core::object_store::tree_hash_of_commit(h).unwrap_or_default(), "", &mut tree);
                tree
            }).unwrap_or_default();
            let is_file = tree_of.contains_key(&clean);
            let is_dir = !clean.is_empty() && tree_of.keys().any(|p| p.starts_with(&format!("{}/", clean)));
            if is_file || is_dir {
                Target::Path { reference, path: clean, dir: is_dir && !is_file, lines: if is_file { lines } else { None } }
            } else if refs::read_ref(&format!("refs/heads/{}", t))?.is_some() || refs::read_ref(&format!("refs/remotes/{}/{}", remote, t))?.is_some() {
                if !on_remote(t) {
                    crate::ui::die_fatal(format!("'{}' isn't on {} yet, so it has no page there\nhint: `rgit push -u {} {}` first", t, remote, remote, t));
                }
                Target::Tree { reference: t.to_string() }
            } else if let Ok(hash) = crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", t)) {
                Target::Commit(hash)
            } else {
                crate::ui::die_fatal(format!("'{}' is not a file, branch or commit here", t));
            }
        }
    };

    let link = url_for(&repo, &chosen);
    println!("{}", link);
    if !print_only && let Err(e) = launch(&link) {
        eprintln!("{}: open the link above yourself", e);
    }
    Ok(())
}
