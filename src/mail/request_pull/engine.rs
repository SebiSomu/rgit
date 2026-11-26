use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use crate::core::config::get_config_value;
use crate::remote::fetch::engine::read_object_from_path;
use crate::core::commit::{parse_commit_object, split_commit_message};
use crate::diff::hunks::diff_tree_file;
use crate::remote::github::{build_github_client, get_github_default_branch, list_github_refs, resolve_github_token};
use crate::remote::transport::{not_a_repository, resolve_remote, types::Transport};
use crate::mail::mailmap::map_user;
use crate::core::object_store::{flatten_tree, read_object};
use crate::core::refs::list_all_refs;
use crate::history::rev_list::engine::run_rev_list_walk;
use crate::core::rev_parse::resolve_rev;
use crate::references::symbolic_ref::engine::resolve_ref_unsafe;
use crate::diff::types::FileDiff;
use crate::history::rev_list::types::{RevListOptions, RevListRoots};
use crate::mail::mailmap::types::Mailmap;
use crate::remote::types::GitHubTarget;
use crate::mail::request_pull::types::RequestPullOptions;

/// git's usage text for `request-pull`, printed for `-h` and when `<start>` or `<url>` is missing.
pub const REQUEST_PULL_USAGE: &str = "usage: rgit request-pull [options] start url [end]

    -p                    show patch text as well

";

/// Parses `request-pull` arguments: `-p`, then `<start> <url> [<end>]`. Errors carry the
/// usage text.
// Used by `request-pull`.
pub fn parse_request_pull_args(args: &[String]) -> anyhow::Result<RequestPullOptions> {
    let mut opts = RequestPullOptions::default();
    let mut positional = Vec::new();
    let mut only_positional = false;
    for arg in args {
        match arg.as_str() {
            "--" if !only_positional => only_positional = true,
            "-p" if !only_positional => opts.patch = true,
            other if !only_positional && other.starts_with('-') && other.len() > 1 => {
                let (kind, name) = if let Some(name) = other.strip_prefix("--") { ("option", name) } else { ("switch", &other[1..2]) };
                anyhow::bail!("error: unknown {} `{}'\n{}", kind, name, REQUEST_PULL_USAGE);
            }
            other => positional.push(other.to_string()),
        }
    }
    let mut positional = positional.into_iter();
    match (positional.next().filter(|s| !s.is_empty()), positional.next().filter(|s| !s.is_empty())) {
        (Some(start), Some(url)) => {
            opts.start = start;
            opts.url = url;
        }
        _ => anyhow::bail!("{}", REQUEST_PULL_USAGE),
    }
    opts.end = positional.next().unwrap_or_default();
    Ok(opts)
}

/// Finds what `<end>`'s local half names, the way git-request-pull does: the target of a
/// symbolic ref (`HEAD` gives `refs/heads/<branch>`), else the branch or tag ref(s) whose
/// name ends in it (several, one per line, when both a branch and a tag match), else the
/// object it resolves to. `None` when it names nothing.
// Used by `request-pull`.
pub fn resolve_pull_head(local: &str) -> Option<String> {
    if let Some(resolved) = resolve_ref_unsafe(local, true, false).filter(|r| r.is_symref) {
        return Some(resolved.name);
    }
    let suffix = format!("/{}", local);
    let matches: Vec<String> = list_all_refs()
        .into_keys()
        .filter(|name| (name.starts_with("refs/heads/") || name.starts_with("refs/tags/")) && (name == local || name.ends_with(&suffix)))
        .collect();
    if !matches.is_empty() {
        return Some(matches.join("\n"));
    }
    resolve_rev(local).ok()
}

/// `git ls-remote --get-url`: the configured URL when `url` is a remote name, else `url` itself.
// Used by `request-pull`.
pub fn remote_display_url(url: &str) -> String {
    get_config_value(&format!("remote.{}.url", url)).ok().flatten().unwrap_or_else(|| url.to_string())
}

/// Lists a repository's refs like `git ls-remote`: `HEAD` first, then every ref in name
/// order, an annotated tag being followed by a `<ref>^{}` entry with the commit it points
/// at. Works for the repositories rgit can talk to: local paths (or remotes configured
/// with one), smart-HTTP servers (GitHub included), and GitHub's REST API, where only
/// branches are listed.
// Used by `request-pull`.
pub fn list_remote_refs_for_pull(url: &str) -> anyhow::Result<Vec<(String, String)>> {
    let git_dir = match resolve_remote(url)?.transport {
        Transport::GitHubApi { owner, repo } => {
            let target = GitHubTarget { owner, repo, token: resolve_github_token(url)? };
            let client = build_github_client(&target.token)?;
            let branches = list_github_refs(&client, &target)?;
            let mut refs = Vec::new();
            if let Ok(default_branch) = get_github_default_branch(&client, &target) {
                if let Some((_, hash)) = branches.iter().find(|(name, _)| *name == default_branch) {
                    refs.push((hash.clone(), "HEAD".to_string()));
                }
            }
            let sorted: BTreeMap<String, String> = branches.into_iter().map(|(name, hash)| (format!("refs/heads/{}", name), hash)).collect();
            refs.extend(sorted.into_iter().map(|(name, hash)| (hash, name)));
            return Ok(refs);
        }
        Transport::Bundle { path } => return Err(not_a_repository(&path)),
        Transport::SmartHttp { url: http_url } => {
            let client = crate::remote::http::HttpClient::for_remote(url, &http_url)?;
            let discovery = crate::remote::smart_http::discover_refs(&client, &http_url, &[], false)?;
            let mut refs = Vec::new();
            if let Some(head) = discovery.refs.iter().find(|r| r.name == "HEAD").and_then(|r| r.oid.clone()) {
                refs.push((head, "HEAD".to_string()));
            }
            let mut named: Vec<_> = discovery.refs.iter().filter(|r| r.name != "HEAD").collect();
            named.sort_by(|a, b| a.name.cmp(&b.name));
            for r in named {
                let Some(oid) = r.oid.clone() else { continue };
                refs.push((oid, r.name.clone()));
                if let Some(peeled) = &r.peeled {
                    refs.push((peeled.clone(), format!("{}^{{}}", r.name)));
                }
            }
            return Ok(refs);
        }
        Transport::Local { git_dir } => git_dir,
    };
    let mut by_name: BTreeMap<String, String> = BTreeMap::new();
    let mut peeled: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(packed) = fs::read_to_string(git_dir.join("packed-refs")) {
        let mut last: Option<String> = None;
        for line in packed.lines() {
            if let Some(hash) = line.strip_prefix('^') {
                if let Some(name) = &last {
                    peeled.insert(name.clone(), hash.trim().to_string());
                }
            } else if let Some((hash, name)) = line.split_once(' ').filter(|(hash, _)| hash.len() == 40) {
                by_name.insert(name.to_string(), hash.to_string());
                last = Some(name.to_string());
            }
        }
    }
    collect_loose_remote_refs(&git_dir.join("refs"), "refs", &mut by_name, &mut peeled);

    let mut refs = Vec::new();
    if let Ok(head) = fs::read_to_string(git_dir.join("HEAD")) {
        let head = head.trim();
        match head.strip_prefix("ref: ") {
            Some(target) => refs.extend(by_name.get(target.trim()).map(|hash| (hash.clone(), "HEAD".to_string()))),
            None if head.len() == 40 => refs.push((head.to_string(), "HEAD".to_string())),
            None => {}
        }
    }
    for (name, hash) in &by_name {
        refs.push((hash.clone(), name.clone()));
        let target = peeled.get(name).cloned().or_else(|| peel_remote_tag(&git_dir, hash));
        if let Some(target) = target.filter(|target| target != hash) {
            refs.push((target, format!("{}^{{}}", name)));
        }
    }
    Ok(refs)
}

/// Reads every loose ref under a remote repository's `refs/` directory; a loose ref
/// replaces a packed one of the same name (and its recorded peeled value).
// Used by `list_remote_refs_for_pull`.
fn collect_loose_remote_refs(dir: &Path, prefix: &str, out: &mut BTreeMap<String, String>, peeled: &mut BTreeMap<String, String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = format!("{}/{}", prefix, entry.file_name().to_string_lossy());
        let path = entry.path();
        if path.is_dir() {
            collect_loose_remote_refs(&path, &name, out, peeled);
        } else if let Ok(content) = fs::read_to_string(&path) {
            let hash = content.trim();
            if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                peeled.remove(&name);
                out.insert(name, hash.to_string());
            }
        }
    }
}

/// Follows annotated tag objects in a remote repository's loose object store down to the
/// first non-tag object; `None` when `hash` is not a (readable) tag.
// Used by `list_remote_refs_for_pull`.
fn peel_remote_tag(git_dir: &Path, hash: &str) -> Option<String> {
    let mut current = hash.to_string();
    for _ in 0..10 {
        let path = git_dir.join("objects").join(&current[..2]).join(&current[2..]);
        let (kind, content) = read_object_from_path(&path).ok()?;
        if kind != "tag" {
            return (current != hash).then_some(current);
        }
        let text = String::from_utf8_lossy(&content).into_owned();
        current = text.lines().find_map(|line| line.strip_prefix("object "))?.trim().to_string();
        if current.len() != 40 {
            return None;
        }
    }
    None
}

/// Port of the `find_matching_ref` Perl snippet in git-request-pull: scans `ls-remote`
/// lines for a ref named `head` (or ending in `/head`) and returns `(remote object, ref
/// name)` when one of them, or its peeled `^{}` entry, is the commit `head_rev`. Like the
/// original, later lines override earlier ones.
// Used by `request-pull`.
pub fn find_matching_remote_ref(refs: &[(String, String)], head: &str, head_rev: &str) -> Option<(String, String)> {
    let suffix = format!("/{}", head);
    let (mut remote_hash, mut found): (Option<String>, Option<String>) = (None, None);
    for (hash, listed) in refs {
        let (name, deref) = match listed.find('^') {
            Some(caret) => (&listed[..caret], &listed[caret..]),
            None => (listed.as_str(), ""),
        };
        if hash == head {
            found = Some(hash.clone());
            remote_hash = Some(hash.clone());
        }
        if name == head || name.ends_with(&suffix) {
            if deref.is_empty() {
                remote_hash = Some(hash.clone());
            }
            if hash == head_rev {
                found = Some(name.to_string());
            }
        }
    }
    found.map(|found| (remote_hash.unwrap_or_else(|| head_rev.to_string()), found))
}

/// Formats `git shortlog ^<exclude> <include>`: the commits of the range grouped by author
/// name (corrected through the mailmap, sorted by name), each group as `Name (count):`
/// followed by the subjects oldest first, indented six spaces, and a blank line. A leading
/// `[PATCH ...]` is dropped from subjects and an empty one shows as `<none>`.
// Used by `request-pull`.
pub fn format_range_shortlog(exclude: &str, include: &str, mailmap: &Mailmap) -> anyhow::Result<String> {
    let mut roots = RevListRoots::default();
    roots.pending.push((exclude.to_string(), true, false));
    roots.pending.push((include.to_string(), false, false));
    roots.bottoms.insert(exclude.to_string());
    let (walked, state) = run_rev_list_walk(&RevListOptions::default(), &roots, &|_, _| true)?;

    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for w in walked.iter().filter(|w| !w.hidden && !state.uninteresting.contains(&w.commit.hash)) {
        let author = map_user(mailmap, w.commit.author.name.trim(), &w.commit.author.email).0;
        let subject = split_commit_message(&w.commit.message).0;
        let mut oneline = subject.trim_start();
        if oneline.starts_with("[PATCH") {
            if let Some(end) = oneline.find(']') {
                oneline = oneline[end + 1..].trim_start();
            }
        }
        groups.entry(author).or_default().push(if oneline.is_empty() { "<none>".to_string() } else { oneline.to_string() });
    }

    let mut out = String::new();
    for (author, subjects) in &groups {
        out.push_str(&format!("{} ({}):\n", author, subjects.len()));
        for subject in subjects.iter().rev() {
            out.push_str(&format!("      {}\n", subject));
        }
        out.push('\n');
    }
    Ok(out)
}

/// Diffs the trees of two commits path by path (no rename detection): one git-format file
/// diff, with its diffstat entry and summary line, for every path that differs.
// Used by `request-pull`.
pub fn diff_commit_trees(old_commit: &str, new_commit: &str) -> anyhow::Result<Vec<FileDiff>> {
    let (mut old_tree, mut new_tree) = (BTreeMap::new(), BTreeMap::new());
    flatten_tree(&parse_commit_object(old_commit)?.tree, "", &mut old_tree)?;
    flatten_tree(&parse_commit_object(new_commit)?.tree, "", &mut new_tree)?;

    let paths: BTreeSet<&String> = old_tree.keys().chain(new_tree.keys()).collect();
    let mut diffs = Vec::new();
    for path in paths {
        let (old, new) = (old_tree.get(path).copied(), new_tree.get(path).copied());
        if old != new {
            diffs.push(diff_tree_file(path, old, new)?);
        }
    }
    Ok(diffs)
}

/// The message of an annotated tag as git-request-pull prints it: everything after the
/// header's blank line, up to a PGP/SSH signature block. `None` when the object isn't a tag.
// Used by `request-pull`.
pub fn tag_message_for_pull(object: &str) -> Option<String> {
    let hash = resolve_rev(object).ok()?;
    let (kind, content) = read_object(&hash).ok()?;
    if kind != "tag" {
        return None;
    }
    let text = String::from_utf8_lossy(&content).into_owned();
    let body = text.split_once("\n\n").map(|(_, body)| body).unwrap_or("");
    let mut out = String::new();
    for line in body.lines() {
        if line.starts_with("-----BEGIN PGP ") || line.starts_with("-----BEGIN SSH ") || line.starts_with("-----BEGIN SIGNED ") {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

