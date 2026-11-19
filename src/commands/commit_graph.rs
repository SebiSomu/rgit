use std::fs;
use std::io::{self, BufRead};
use anyhow::Result;

use crate::helpers::commit_graph::{commit_graph_commits, commit_graph_path, verify_commit_graph, write_commit_graph};
use crate::helpers::objects::read_object;
use crate::helpers::pack::repo_packs;
use crate::helpers::rev_list::list_all_refs;
use crate::helpers::rev_parse::{peel_to_type, resolve_rev};
use crate::repo;

/// `rgit commit-graph write [--reachable | --stdin-commits | --stdin-packs] [--append]` /
/// `rgit commit-graph verify`
///
/// Writes or checks the commit-graph file (`.git/objects/info/commit-graph`), git's
/// precomputed table of every commit's tree, parents, commit time and generation number,
/// which lets git walk history without opening commit objects. `write` covers the commits
/// found in packfiles (git's default), every commit reachable from the refs with
/// `--reachable`, the commits listed on standard input with `--stdin-commits`, or those in
/// the pack indexes named on standard input with `--stdin-packs` — always together with all
/// their ancestors. `--append` keeps the commits already in the file. Nothing is written
/// when there are no commits. `verify` re-reads the file and compares every row with the
/// object store, printing what does not match. The file is in git's format (version 1 with
/// generation data), so git can use it; rgit itself does not read it yet. Split graphs
/// (`--split`) and changed-path Bloom filters (`--changed-paths`) are not supported.
/// Exit status: 0, 1 when `verify` finds problems, 128 on errors, 129 for a usage error.
pub fn commit_graph(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit commit-graph verify\n   or: rgit commit-graph write [--append] [--reachable | --stdin-packs | --stdin-commits]\n";
    let die = |message: &str, code: i32| -> ! {
        eprint!("{}", message);
        crate::ui::exit(code);
    };
    let (mut reachable, mut stdin_commits, mut stdin_packs, mut append) = (false, false, false, false);
    let mut action: Option<&str> = None;
    for arg in &args {
        match arg.as_str() {
            "write" | "verify" if action.is_none() => action = Some(arg.as_str()),
            "--reachable" => reachable = true,
            "--stdin-commits" => stdin_commits = true,
            "--stdin-packs" => stdin_packs = true,
            "--append" => append = true,
            "--progress" | "--no-progress" | "--shallow" | "--no-changed-paths" => {}
            "--changed-paths" => eprintln!("warning: --changed-paths is ignored: rgit does not write Bloom filters"),
            "-h" => {
                print!("{}", USAGE);
                crate::ui::exit(129);
            }
            other if other.starts_with("--object-dir") => die("fatal: --object-dir is not supported by rgit commit-graph\n", 128),
            other if other.starts_with("--split") || other.starts_with("--max-commits") || other.starts_with("--size-multiple") || other.starts_with("--expire-time") => {
                die("fatal: split commit-graphs are not supported by rgit\n", 128)
            }
            other => die(&format!("error: unknown option `{}'\n{}", other.trim_start_matches('-'), USAGE), 129),
        }
    }
    let Some(action) = action else { die(USAGE, 129) };
    if !repo::exists() {
        die("fatal: not a git repository (or any of the parent directories): .git\n", 128);
    }
    let path = commit_graph_path();

    if action == "verify" {
        let Ok(data) = fs::read(&path) else { return Ok(()) };
        let problems = verify_commit_graph(&data);
        for problem in &problems {
            eprintln!("error: {}", problem);
        }
        if !problems.is_empty() {
            crate::ui::exit(1);
        }
        return Ok(());
    }

    if [reachable, stdin_commits, stdin_packs].iter().filter(|on| **on).count() > 1 {
        die("fatal: use at most one of --reachable, --stdin-commits, or --stdin-packs\n", 128);
    }
    let mut starts: Vec<String> = Vec::new();
    if reachable {
        for hash in list_all_refs().into_values() {
            if let Ok(commit) = peel_to_type(hash, "commit") {
                starts.push(commit);
            }
        }
        if let Some(head) = crate::refs::resolve_head_commit()? {
            starts.push(head);
        }
    } else if stdin_commits {
        for line in io::stdin().lock().lines() {
            let line = line?;
            let name = line.trim();
            if name.is_empty() {
                continue;
            }
            match resolve_rev(name).and_then(|hash| peel_to_type(hash, "commit")) {
                Ok(commit) => starts.push(commit),
                Err(_) => die(&format!("fatal: unexpected non-hex object ID: {}\n", name), 128),
            }
        }
    } else {
        // Commits in packfiles: all packs, or the pack indexes named on standard input.
        let wanted: Option<Vec<String>> = if stdin_packs {
            Some(io::stdin().lock().lines().map_while(|l| l.ok()).map(|l| l.trim().trim_end_matches(".idx").trim_end_matches(".pack").to_string()).filter(|l| !l.is_empty()).collect())
        } else {
            None
        };
        for pack in repo_packs().iter() {
            if wanted.as_ref().is_some_and(|names| !names.iter().any(|name| name.ends_with(&pack.name))) {
                continue;
            }
            for hash in &pack.index.hashes {
                let hash = hex::encode(hash);
                if matches!(read_object(&hash), Ok((ref kind, _)) if kind == "commit") {
                    starts.push(hash);
                }
            }
        }
    }
    if append {
        starts.extend(commit_graph_commits());
    }
    if let Some((bytes, _)) = write_commit_graph(&starts)? {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        if path.exists() {
            let mut permissions = fs::metadata(&path)?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(&path, permissions)?;
        }
        fs::write(&path, bytes)?;
    }
    Ok(())
}
