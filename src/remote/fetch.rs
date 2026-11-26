pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;
use crate::remote::fetch::engine::{
    enumerate_objects_to_fetch,
    fetch_object_from_remote,
    list_remote_refs,
    read_local_remote_tracking_ref,
    read_remote_head,
    write_fetch_head,
    write_local_remote_tracking_ref,
};
use crate::remote::bundle::engine::{bundle_branches, bundle_head_branch, check_bundle_prerequisites, read_bundle, read_packfile, store_loose_objects};
use crate::remote::github::{fetch_from_github, resolve_github_token};
use crate::remote::transport::{resolve_remote, types::Transport};
use crate::remote::{fetch::types::{FetchRefResult, TagMode}, types::GitHubTarget};

pub fn fetch(remote: Option<String>, branch: Option<String>) -> Result<()> {
    fetch_with_tags(remote, branch, TagMode::Follow)
}

/// `fetch` with a choice of tags (`--tags`, `--no-tags`; the default follows tags, as git
/// does). Tags are fetched only over HTTPS so far; other transports ignore the choice.
// Used by `fetch` and `rgit fetch`.
pub fn fetch_with_tags(remote: Option<String>, branch: Option<String>, tags: TagMode) -> Result<()> {
    let remote_name = match remote {
        Some(r) => r,
        None => {
            if let Ok(Some(b)) = crate::core::refs::resolve_head()
                .map(|h| match h {
                    crate::core::refs::HeadState::Branch(b) => Some(b),
                    _ => None,
                })
            {
                crate::core::config::get_config_value(&format!("branch.{}.remote", b))?
                    .unwrap_or_else(|| "origin".to_string())
            } else {
                "origin".to_string()
            }
        }
    };

    let remote = resolve_remote(&remote_name)?;
    let remote_display = remote.display();
    let remote_git_dir = match remote.transport {
        Transport::GitHubApi { owner, repo } => {
            let token = resolve_github_token(&remote_name)?;
            let target = GitHubTarget { owner, repo, token };
            fetch_from_github(&target, &remote_name, &remote_display, branch.as_deref())?;
            return Ok(());
        }
        Transport::Bundle { path } => return fetch_from_bundle(&path, &remote_name, &remote_display, branch.as_deref()),
        Transport::SmartHttp { url } => return fetch_from_smart_http(&url, &remote_name, &remote_display, branch.as_deref(), tags),
        Transport::Local { git_dir } => git_dir,
    };

    let remote_refs = list_remote_refs(&remote_git_dir)?;
    let filtered_refs: Vec<_> = match &branch {
        Some(b) => remote_refs
            .into_iter()
            .filter(|(name, _)| name == b)
            .collect(),
        None => remote_refs,
    };

    if filtered_refs.is_empty() {
        println!("From {}", remote_display);
        println!(" * (no branches found or empty repository)");
        return Ok(());
    }

    println!("From {}", remote_display);

    let mut total_objects = 0usize;
    let mut results: Vec<FetchRefResult> = Vec::new();
    let mut fetch_head_entries = Vec::new();

    for (branch_name, remote_sha) in &filtered_refs {
        let local_sha =
            read_local_remote_tracking_ref(&remote_name, branch_name);

        if local_sha.as_deref() == Some(remote_sha.as_str()) {
            println!(
                " = [up to date]      {:20} -> {}/{}",
                branch_name, remote_name, branch_name
            );
            results.push(FetchRefResult::UpToDate {
                branch: branch_name.clone(),
            });
            fetch_head_entries.push((
                remote_sha.clone(),
                branch_name.clone(),
                remote_display.clone(),
            ));
            continue;
        }

        let missing = enumerate_objects_to_fetch(remote_sha, &remote_git_dir)?;
        let count = missing.len();
        total_objects += count;

        for hash in &missing {
            fetch_object_from_remote(hash, &remote_git_dir)?;
        }

        write_local_remote_tracking_ref(&remote_name, branch_name, remote_sha)?;
        fetch_head_entries.push((
            remote_sha.clone(),
            branch_name.clone(),
            remote_display.clone(),
        ));

        let result = match &local_sha {
            None => {
                println!(
                    " * [new branch]      {:20} -> {}/{}",
                    branch_name, remote_name, branch_name
                );
                FetchRefResult::NewBranch {
                    branch: branch_name.clone(),
                    sha: remote_sha.clone(),
                }
            }
            Some(old) => {
                println!(
                    "   {}..{}  {:16} -> {}/{}",
                    crate::core::commit::short_hash(old),
                    crate::core::commit::short_hash(remote_sha),
                    branch_name, remote_name, branch_name
                );
                FetchRefResult::Updated {
                    branch: branch_name.clone(),
                    old_sha: old.clone(),
                    new_sha: remote_sha.clone(),
                }
            }
        };
        results.push(result);
    }

    if total_objects > 0 {
        println!(
            "Receiving objects: 100% ({}/{}), done.",
            total_objects, total_objects
        );
    }

    if let Ok(Some(default_branch)) = read_remote_head(&remote_git_dir) {
        if let Some((_, sha)) = filtered_refs.iter().find(|(b, _)| b == &default_branch) {
            let _ = crate::core::refs::write_remote_head(&remote_name, &default_branch, sha);
        }
    }

    write_fetch_head(&fetch_head_entries)?;

    Ok(())
}


/// `rgit fetch <bundle>` (or a remote whose url is a bundle file): checks the bundle's
/// prerequisites, stores its objects here, then updates `refs/remotes/<remote>/<branch>` for
/// each branch it carries (only `branch` when given), creates the tags it carries that don't
/// exist here, and writes its remote HEAD and FETCH_HEAD, with the same output as a fetch from
/// a local repository.
fn fetch_from_bundle(path: &std::path::Path, remote_name: &str, remote_display: &str, branch: Option<&str>) -> Result<()> {
    let (header, bytes) = read_bundle(&path.to_string_lossy())?;
    check_bundle_prerequisites(&header)?;
    let branches: Vec<(String, String)> = bundle_branches(&header).into_iter().filter(|(name, _)| branch.is_none_or(|b| b == name)).collect();

    println!("From {}", remote_display);
    if branches.is_empty() {
        println!(" * (no branches found or empty repository)");
        return Ok(());
    }

    let mut received = 0usize;
    let mut fetch_head_entries = Vec::new();
    for (branch_name, remote_sha) in &branches {
        fetch_head_entries.push((remote_sha.clone(), branch_name.clone(), remote_display.to_string()));
        let local_sha = read_local_remote_tracking_ref(remote_name, branch_name);
        if local_sha.as_deref() == Some(remote_sha.as_str()) {
            println!(" = [up to date]      {:20} -> {}/{}", branch_name, remote_name, branch_name);
            continue;
        }
        // The objects are stored once, before the first ref that needs them.
        if received == 0 {
            received = store_loose_objects(&read_packfile(&bytes[header.pack_offset..])?)?;
        }
        write_local_remote_tracking_ref(remote_name, branch_name, remote_sha)?;
        match &local_sha {
            None => println!(" * [new branch]      {:20} -> {}/{}", branch_name, remote_name, branch_name),
            Some(old) => println!(
                "   {}..{}  {:16} -> {}/{}",
                crate::core::commit::short_hash(old),
                crate::core::commit::short_hash(remote_sha),
                branch_name, remote_name, branch_name
            ),
        }
    }

    // Tags come along too (like git's clone and its tag following): new ones are created,
    // existing ones are left alone.
    for (oid, refname) in &header.refs {
        let Some(tag) = refname.strip_prefix("refs/tags/") else { continue };
        match crate::core::refs::read_ref(refname)? {
            Some(local) if local == *oid => {}
            Some(_) => println!(" ! [rejected]        {:20} -> {}  (would clobber existing tag)", tag, tag),
            None => {
                if received == 0 {
                    received = store_loose_objects(&read_packfile(&bytes[header.pack_offset..])?)?;
                }
                crate::core::refs::write_ref(refname, oid)?;
                println!(" * [new tag]         {:20} -> {}", tag, tag);
            }
        }
    }

    if received > 0 {
        println!("Receiving objects: 100% ({}/{}), done.", received, received);
    }
    if let Some(default_branch) = bundle_head_branch(&header)
        && let Some((_, sha)) = branches.iter().find(|(b, _)| *b == default_branch)
    {
        let _ = crate::core::refs::write_remote_head(remote_name, &default_branch, sha);
    }
    write_fetch_head(&fetch_head_entries)?;
    Ok(())
}

/// `rgit fetch` from a remote reached over git's smart-HTTP protocol (any https host, and
/// GitHub unless `rgit.httpTransport=api`). Lists the server's branches (and tags, unless
/// `--no-tags`), negotiates with the local ref tips so the server sends only what this
/// repository lacks, then verifies and indexes the pack (completing a thin pack with the
/// local bases it deltas against), checks that every wanted object arrived and that
/// everything the new objects reference exists, and only then installs the pack and moves
/// `refs/remotes/<remote>/<branch>` (only `branch` when given), the remote HEAD and FETCH_HEAD.
/// Tags follow as in git: by default the new tags that point at something this repository
/// has afterwards (the server includes their tag objects in the pack), with `--tags` all new
/// tags; an existing local tag is never changed. A transfer that fails at any point leaves
/// the repository unchanged. The output matches a fetch from a local repository.
fn fetch_from_smart_http(url: &str, remote_name: &str, remote_display: &str, branch: Option<&str>, tags: TagMode) -> Result<()> {
    use crate::remote::fetch::engine::local_has_object;
    use crate::remote::smart_http::discover_refs;

    let client = crate::remote::http::HttpClient::for_remote(remote_name, url)?;
    let mut prefixes = vec!["HEAD".to_string(), "refs/heads/".to_string()];
    if tags != TagMode::None {
        prefixes.push("refs/tags/".to_string());
    }
    let discovery = discover_refs(&client, url, &prefixes, false)?;
    let branches: Vec<(String, String)> = discovery
        .refs
        .iter()
        .filter_map(|r| Some((r.name.strip_prefix("refs/heads/")?.to_string(), r.oid.clone()?)))
        .filter(|(name, _)| branch.is_none_or(|b| b == name))
        .collect();

    let shown = crate::remote::http::redact_url(remote_display);
    println!("From {}", shown);
    if branches.is_empty() {
        println!(" * (no branches found or empty repository)");
        return Ok(());
    }

    // The server's tags this repository doesn't have yet: (name, tag value, what it points at).
    let local_refs = crate::core::refs::list_all_refs();
    let mut clobbered: Vec<String> = Vec::new();
    let candidate_tags: Vec<(String, String, String)> = if tags == TagMode::None {
        Vec::new()
    } else {
        discovery
            .refs
            .iter()
            .filter_map(|r| {
                let name = r.name.strip_prefix("refs/tags/")?;
                let oid = r.oid.clone()?;
                match local_refs.get(&r.name) {
                    Some(local) if *local == oid => return None,
                    // An existing tag is never moved; `--tags` reports it, as git does.
                    Some(_) => {
                        if tags == TagMode::All {
                            clobbered.push(name.to_string());
                        }
                        return None;
                    }
                    None => {}
                }
                let target = r.peeled.clone().unwrap_or_else(|| oid.clone());
                Some((name.to_string(), oid, target))
            })
            .collect()
    };

    let mut wants: Vec<String> = branches.iter().map(|(_, oid)| oid.clone()).collect();
    // `--tags` wants every new tag; following wants the tags whose target is already here.
    for (_, oid, target) in &candidate_tags {
        if tags == TagMode::All || local_has_object(target) {
            wants.push(oid.clone());
        }
    }
    wants.retain(|oid| !local_has_object(oid));
    wants.sort();
    wants.dedup();
    let mut received = download_pack(&client, &discovery, url, &shown, &wants, tags == TagMode::Follow)?;
    // Without include-tag the server may have left out tag objects of fetched commits.
    let missing_tags: Vec<String> = candidate_tags
        .iter()
        .filter(|(_, oid, target)| local_has_object(target) && !local_has_object(oid))
        .map(|(_, oid, _)| oid.clone())
        .collect();
    if !missing_tags.is_empty() {
        received += download_pack(&client, &discovery, url, &shown, &missing_tags, false)?;
    }

    let mut fetch_head_entries = Vec::new();
    for (branch_name, remote_sha) in &branches {
        fetch_head_entries.push((remote_sha.clone(), branch_name.clone(), shown.clone()));
        let local_sha = read_local_remote_tracking_ref(remote_name, branch_name);
        if local_sha.as_deref() == Some(remote_sha.as_str()) {
            println!(" = [up to date]      {:20} -> {}/{}", branch_name, remote_name, branch_name);
            continue;
        }
        write_local_remote_tracking_ref(remote_name, branch_name, remote_sha)?;
        match &local_sha {
            None => println!(" * [new branch]      {:20} -> {}/{}", branch_name, remote_name, branch_name),
            Some(old) => println!(
                "   {}..{}  {:16} -> {}/{}",
                crate::core::commit::short_hash(old),
                crate::core::commit::short_hash(remote_sha),
                branch_name, remote_name, branch_name
            ),
        }
    }
    for (name, oid, _) in &candidate_tags {
        if local_has_object(oid) {
            crate::core::refs::write_ref(&format!("refs/tags/{}", name), oid)?;
            println!(" * [new tag]         {:20} -> {}", name, name);
        }
    }
    for name in &clobbered {
        println!(" ! [rejected]        {:20} -> {}  (would clobber existing tag)", name, name);
    }
    if received > 0 {
        println!("Receiving objects: 100% ({}/{}), done.", received, received);
    }
    let head_target = discovery.refs.iter().find(|r| r.name == "HEAD").and_then(|r| r.symref_target.as_deref()?.strip_prefix("refs/heads/"));
    if let Some(default_branch) = head_target
        && let Some((_, sha)) = branches.iter().find(|(b, _)| b == default_branch)
    {
        let _ = crate::core::refs::write_remote_head(remote_name, default_branch, sha);
    }
    write_fetch_head(&fetch_head_entries)?;
    if !clobbered.is_empty() {
        anyhow::bail!("error: some local refs could not be updated; delete those tags first (rgit tag -d <name>) to take the remote's");
    }
    Ok(())
}

/// Downloads, verifies and installs one pack with `wants` (nothing when empty); with
/// `include_tag` the server adds the tag objects of tags pointing into the pack. Returns the
/// number of objects the server sent. Nothing is stored unless the whole pack checks out.
// Used by `fetch_from_smart_http`.
fn download_pack(
    client: &crate::remote::http::HttpClient,
    discovery: &crate::remote::smart_http::types::Discovery,
    url: &str,
    shown: &str,
    wants: &[String],
    include_tag: bool,
) -> Result<usize> {
    use std::io::IsTerminal;
    if wants.is_empty() {
        return Ok(0);
    }
    // The local ref tips, annotated tags peeled to what they name, are where the `have`s start.
    let mut local_tips: Vec<String> = crate::core::refs::list_all_refs()
        .into_values()
        .filter_map(|oid| crate::history::name_rev::engine::peel_ref_tip(&oid).map(|(peeled, _, _, _)| peeled))
        .collect();
    local_tips.sort();
    local_tips.dedup();
    let pack = crate::remote::smart_http::fetch_pack(client, discovery, url, wants, &local_tips, include_tag, std::io::stderr().is_terminal())?;
    let received = pack_object_count(&pack);
    let (pack, index, objects) = crate::core::pack::index_pack_bytes(pack, true)?;
    let arrived: std::collections::HashSet<&str> = objects.iter().map(|o| o.hash.as_str()).collect();
    if let Some(missing) = wants.iter().find(|w| !arrived.contains(w.as_str())) {
        anyhow::bail!("fatal: the pack from '{}' does not contain {}; nothing was updated", shown, missing);
    }
    if let Some((object, missing)) = crate::remote::fetch::engine::first_missing_reference(&objects, &arrived) {
        anyhow::bail!("fatal: the pack from '{}' is incomplete: {} references {}, which is missing; nothing was updated", shown, object, missing);
    }
    crate::core::pack::store_pack(&crate::core::repo::path(".git/objects/pack"), "pack-", &pack, &index)?;
    Ok(received)
}

/// The number of objects a pack's header announces (what the server sent, before a thin
/// pack is completed with local bases); 0 for a pack too short to have a header.
// Used by `fetch_from_smart_http`.
fn pack_object_count(pack: &[u8]) -> usize {
    pack.get(8..12).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize).unwrap_or(0)
}
