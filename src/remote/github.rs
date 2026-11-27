pub mod types;

use anyhow::{Context, Result};
use std::io::IsTerminal;
use base64::Engine;
use std::collections::{HashMap, HashSet, VecDeque};
use crate::remote::{push::types::PushStatus, types::GitHubTarget};
use crate::remote::github::types::{CommitIdentity, GitHubTreeEntry};

/// Converts a Unix timestamp in seconds since epoch to an ISO 8601 UTC formatted string.
// Used by `github`.
pub fn format_unix_timestamp_iso8601(secs: u64) -> String {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;

    let z = days as i64 + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_month = if mp < 10 { mp + 3 } else { mp - 9 };
    let y_year = if m_month <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y_year, m_month, d, h, m, s)
}

/// Parses an author or committer line from a commit object into structured identity data.
// Used by `github`.
pub fn parse_commit_identity(line: &str) -> Option<CommitIdentity> {
    let content = line.strip_prefix("author ")
        .or_else(|| line.strip_prefix("committer "))?;

    let open_bracket = content.find('<')?;
    let close_bracket = content.find('>')?;
    if close_bracket <= open_bracket {
        return None;
    }

    let name = content[..open_bracket].trim().to_string();
    let email = content[open_bracket + 1..close_bracket].trim().to_string();

    let rest = content[close_bracket + 1..].trim();
    let mut fields = rest.split_whitespace();
    let secs: u64 = fields.next()?.parse().unwrap_or(0);
    let offset_minutes = fields.next().and_then(parse_tz_offset).unwrap_or(0);
    let date = format_iso8601_with_offset(secs, offset_minutes);

    Some(CommitIdentity { name, email, date })
}

/// Parses a git time zone (`+0200`, `-0530`) into minutes east of UTC.
// Used by `parse_commit_identity`.
fn parse_tz_offset(tz: &str) -> Option<i64> {
    let (sign, digits) = match tz.as_bytes().first()? {
        b'+' => (1, &tz[1..]),
        b'-' => (-1, &tz[1..]),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok()?;
    let minutes: i64 = digits[2..].parse().ok()?;
    Some(sign * (hours * 60 + minutes))
}

/// An ISO 8601 date in the commit's own time zone (`2026-11-26T12:24:00+02:00`, or `...Z` at
/// UTC), so GitHub rebuilds the commit with the same zone and therefore the same hash.
// Used by `parse_commit_identity`.
pub fn format_iso8601_with_offset(secs: u64, offset_minutes: i64) -> String {
    if offset_minutes == 0 {
        return format_unix_timestamp_iso8601(secs);
    }
    let local = (secs as i64 + offset_minutes * 60).max(0) as u64;
    let utc_text = format_unix_timestamp_iso8601(local);
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let abs = offset_minutes.abs();
    format!("{}{}{:02}:{:02}", utc_text.trim_end_matches('Z'), sign, abs / 60, abs % 60)
}

/// A commit's message exactly as stored: everything after the blank line that ends the
/// headers, trailing newline included. GitHub keeps the message byte for byte, so trimming
/// it (as `extract_commit_message` does for display) would change the commit's hash.
// Used by `push_to_github`.
pub fn raw_commit_message(commit_text: &str) -> &str {
    commit_text.split_once("\n\n").map(|(_, message)| message).unwrap_or("")
}

/// Fails a push when GitHub stored a commit under a different hash than the local one: the
/// API rebuilds commits from their parts, and anything it can't reproduce (a signature, an
/// unusual header or tree) changes the hash. Run before any ref is updated.
// Used by `push_to_github`.
pub fn ensure_pushed_commit_matches(local: &str, remote: &str, branch: &str) -> Result<()> {
    if local == remote {
        return Ok(());
    }
    anyhow::bail!(
        "fatal: GitHub stored commit {} as {}, which is a different commit hash.\n\
         The GitHub API rebuilds each commit from its parts and could not reproduce this one exactly\n\
         (for example a signature, an unusual header, or a tree written in the wrong order).\n\
         Nothing was updated: '{}' on GitHub and its remote-tracking branch are unchanged.\n\
         hint: `git push` sends the commit as it is.",
        &local[..local.len().min(7)], &remote[..remote.len().min(7)], branch
    )
}

/// Inspects a remote URL or remote spec to determine if it points to a GitHub repository,
/// extracting the repository owner and name.
// Used by `push`.
pub fn parse_github_remote(remote_url: &str) -> Option<(String, String)> {
    let clean = remote_url.trim().trim_end_matches(".git");

    if let Some(rest) = clean.strip_prefix("git@github.com:") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    if let Some(idx) = clean.find("github.com/") {
        let rest = &clean[idx + "github.com/".len()..];
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    if !clean.starts_with('.') && !clean.starts_with('/') && !clean.starts_with('\\') && !clean.contains(':') {
        let parts: Vec<&str> = clean.split('/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            if !std::path::Path::new(clean).exists() {
                return Some((parts[0].to_string(), parts[1].to_string()));
            }
        }
    }

    None
}

/// Retrieves the GitHub personal access token: `GITHUB_TOKEN` / `RGIT_GITHUB_TOKEN`, then
/// `remote.<name>.token` / `github.token`, then the credential helpers and a prompt (the
/// password of the `https://github.com` login).
// Used by `push`, `fetch`, `clone` and `request-pull` over the GitHub REST API.
pub fn resolve_github_token(remote_name: &str) -> Result<String> {
    if let Some(token) = configured_github_token(remote_name)? {
        return Ok(token);
    }

    // Then the credential helpers (`credential.helper`), and a prompt when someone can answer
    // it: the login's password is the token. It is approved or rejected by the first request.
    let mut credential = crate::remote::credential::types::Credential::for_https_host("github.com");
    use crate::remote::credential::engine::{askpass_program, terminal_prompts_disabled};
    let can_prompt = askpass_program().is_some() || (std::io::stdin().is_terminal() && std::io::stderr().is_terminal() && !terminal_prompts_disabled());
    let typed = crate::remote::credential::engine::fill(&mut credential, can_prompt).map_err(|message| anyhow::anyhow!("fatal: {}", message))?;
    if let Some(token) = credential.password.clone().filter(|t| !t.trim().is_empty()) {
        crate::remote::credential::engine::set_pending(credential, typed);
        return Ok(token.trim().to_string());
    }

    anyhow::bail!(
        "fatal: GitHub token not found.\n\
         Please provide a Personal Access Token with repo scope via:\n\
         - Setting environment variable GITHUB_TOKEN\n\
         - Or configuring via: rgit config github.token <your-token>\n\
         - Or a credential helper (rgit config --global credential.helper store), then run the command in a terminal once"
    )
}

/// The GitHub token set up without a credential helper, if any: `GITHUB_TOKEN`, then
/// `RGIT_GITHUB_TOKEN`, then `remote.<name>.token`, then `github.token` (empty values are
/// skipped). Never prompts.
// Used by `resolve_github_token` and `HttpClient::for_remote` (the login for github.com).
pub fn configured_github_token(remote_name: &str) -> Result<Option<String>> {
    for variable in ["GITHUB_TOKEN", "RGIT_GITHUB_TOKEN"] {
        if let Ok(token) = std::env::var(variable)
            && !token.trim().is_empty()
        {
            return Ok(Some(token.trim().to_string()));
        }
    }
    for key in [format!("remote.{}.token", remote_name), "github.token".to_string()] {
        if let Some(token) = crate::core::config::get_config_value(&key)?
            && !token.trim().is_empty()
        {
            return Ok(Some(token.trim().to_string()));
        }
    }
    Ok(None)
}

/// Constructs a preconfigured blocking HTTP client with standard GitHub API headers.
// Used by `push`.
pub fn build_github_client(token: &str) -> Result<reqwest::blocking::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(concat!("rgit/", env!("CARGO_PKG_VERSION"))),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
    );
    let auth_val = format!("Bearer {}", token);
    let mut auth_header = reqwest::header::HeaderValue::from_str(&auth_val)
        .context("Invalid characters in GitHub token")?;
    auth_header.set_sensitive(true);
    headers.insert(reqwest::header::AUTHORIZATION, auth_header);

    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .build()
        .context("Failed to initialize HTTP client for GitHub API")
}

/// Fetches the current commit SHA pointed to by a remote branch on GitHub.
// Used by `push`.
pub fn get_github_branch_commit(client: &reqwest::blocking::Client, target: &GitHubTarget, branch: &str) -> Result<Option<String>> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/git/ref/heads/{}",
        target.owner, target.repo, branch
    );
    let res = client.get(&url).send().context("Failed to connect to GitHub API")?;

    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        crate::remote::credential::engine::reject_pending();
        anyhow::bail!("fatal: Authentication failed for GitHub (bad credentials). Check your token (GITHUB_TOKEN, github.token or your credential helper).");
    }
    if res.status() == reqwest::StatusCode::FORBIDDEN {
        anyhow::bail!("fatal: GitHub API access forbidden or rate limit exceeded for repo '{}/{}'.", target.owner, target.repo);
    }
    if !res.status().is_success() {
        let err_body = res.text().unwrap_or_default();
        anyhow::bail!("fatal: GitHub API error ({}): {}", url, err_body);
    }

    crate::remote::credential::engine::approve_pending();
    let json: serde_json::Value = res.json().context("Failed to parse GitHub API ref response")?;
    let sha = json["object"]["sha"]
        .as_str()
        .context("Malformed GitHub ref response: missing object.sha")?
        .to_string();

    Ok(Some(sha))
}

/// Uploads an uncompressed Git blob object to GitHub via the Git Data API.
// Used by `push`.
pub fn upload_blob_to_github(client: &reqwest::blocking::Client, target: &GitHubTarget, blob_hash: &str) -> Result<String> {
    let (obj_type, content) = crate::core::object_store::read_object(blob_hash)?;
    if obj_type != "blob" {
        anyhow::bail!("Expected blob object for hash {}, found {}", blob_hash, obj_type);
    }

    let b64 = base64::engine::general_purpose::STANDARD.encode(&content);
    let url = format!("https://api.github.com/repos/{}/{}/git/blobs", target.owner, target.repo);

    let body = serde_json::json!({
        "content": b64,
        "encoding": "base64"
    });

    let res = client.post(&url).json(&body).send().context("Failed to upload blob to GitHub")?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("Failed to create blob {} on GitHub: {}", blob_hash, err);
    }

    let json: serde_json::Value = res.json()?;
    let sha = json["sha"].as_str().context("Missing sha in blob response")?.to_string();
    Ok(sha)
}

/// Marks every tree and blob reachable from `tree_hash` as already on GitHub (each tree maps
/// to itself: GitHub stores it under the same hash). `read` returns an object's type and
/// content. Submodule entries (gitlinks) are skipped: their commits live in another repository.
// Used by `push_to_github`, seeded with the remote branch's tree.
pub fn collect_known_tree_objects<R: Fn(&str) -> Result<(String, Vec<u8>)>>(
    tree_hash: &str,
    read: &R,
    trees: &mut HashMap<String, String>,
    blobs: &mut HashSet<String>,
) -> Result<()> {
    if trees.contains_key(tree_hash) {
        return Ok(());
    }
    let (kind, content) = read(tree_hash)?;
    if kind != "tree" {
        anyhow::bail!("Expected tree object for hash {}, found {}", tree_hash, kind);
    }
    trees.insert(tree_hash.to_string(), tree_hash.to_string());
    let mut pos = 0;
    while pos < content.len() {
        let space = content[pos..].iter().position(|&b| b == b' ').context("Missing mode separator in tree object")? + pos;
        let nul = content[space..].iter().position(|&b| b == 0).context("Missing name terminator in tree object")? + space;
        let end = nul + 21;
        if end > content.len() {
            anyhow::bail!("Corrupted tree object: incomplete entry hash");
        }
        let mode = &content[pos..space];
        let hash = hex::encode(&content[nul + 1..end]);
        match mode {
            b"40000" | b"040000" => collect_known_tree_objects(&hash, read, trees, blobs)?,
            b"160000" => {}
            _ => {
                blobs.insert(hash);
            }
        }
        pos = end;
    }
    Ok(())
}

/// Recursively traverses and uploads a local tree object and its subtrees/blobs to GitHub.
// Used by `push`.
pub fn upload_tree_to_github(client: &reqwest::blocking::Client, target: &GitHubTarget, tree_hash: &str, uploaded_blobs: &mut HashSet<String>, uploaded_trees: &mut HashMap<String, String>) -> Result<String> {
    if let Some(cached_sha) = uploaded_trees.get(tree_hash) {
        return Ok(cached_sha.clone());
    }

    let (obj_type, content) = crate::core::object_store::read_object(tree_hash)?;
    if obj_type != "tree" {
        anyhow::bail!("Expected tree object for hash {}, found {}", tree_hash, obj_type);
    }

    let mut tree_entries: Vec<GitHubTreeEntry> = Vec::new();
    let mut pos = 0;
    while pos < content.len() {
        let space_pos = content[pos..]
            .iter()
            .position(|&b| b == b' ')
            .context("Missing mode separator in tree object")?
            + pos;
        let mode_raw = String::from_utf8_lossy(&content[pos..space_pos]);

        let null_pos = content[space_pos..]
            .iter()
            .position(|&b| b == 0)
            .context("Missing name terminator in tree object")?
            + space_pos;
        let name = String::from_utf8_lossy(&content[space_pos + 1..null_pos]).to_string();

        let hash_start = null_pos + 1;
        let hash_end = hash_start + 20;
        if hash_end > content.len() {
            anyhow::bail!("Corrupted tree object: incomplete entry hash");
        }
        let sha_hex = hex::encode(&content[hash_start..hash_end]);

        if mode_raw == "40000" || mode_raw == "040000" {
            let child_tree_sha = upload_tree_to_github(
                client,
                target,
                &sha_hex,
                uploaded_blobs,
                uploaded_trees,
            )?;
            tree_entries.push(GitHubTreeEntry {
                path: name,
                mode: "040000".to_string(),
                r#type: "tree".to_string(),
                sha: child_tree_sha,
            });
        } else {
            let normalized_mode = if mode_raw == "100755" { "100755" } else { "100644" };
            if uploaded_blobs.insert(sha_hex.clone()) {
                upload_blob_to_github(client, target, &sha_hex)?;
            }
            tree_entries.push(GitHubTreeEntry {
                path: name,
                mode: normalized_mode.to_string(),
                r#type: "blob".to_string(),
                sha: sha_hex,
            });
        }

        pos = hash_end;
    }

    let url = format!("https://api.github.com/repos/{}/{}/git/trees", target.owner, target.repo);
    let body = serde_json::json!({
        "tree": tree_entries
    });

    let res = client.post(&url).json(&body).send().context("Failed to upload tree to GitHub")?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("Failed to create tree {} on GitHub: {}", tree_hash, err);
    }

    let json: serde_json::Value = res.json()?;
    let remote_tree_sha = json["sha"].as_str().context("Missing sha in tree response")?.to_string();
    uploaded_trees.insert(tree_hash.to_string(), remote_tree_sha.clone());

    Ok(remote_tree_sha)
}

/// Creates a new commit object on GitHub using the Git Data API.
// Used by `push`.
pub fn create_github_commit(client: &reqwest::blocking::Client, target: &GitHubTarget, message: &str, tree_sha: &str, parents: &[String], author: Option<&CommitIdentity>, committer: Option<&CommitIdentity>) -> Result<String> {
    let url = format!("https://api.github.com/repos/{}/{}/git/commits", target.owner, target.repo);

    let mut body = serde_json::json!({
        "message": message,
        "tree": tree_sha,
        "parents": parents,
    });

    if let Some(auth) = author {
        body["author"] = serde_json::json!({
            "name": auth.name,
            "email": auth.email,
            "date": auth.date,
        });
    }
    if let Some(comm) = committer {
        body["committer"] = serde_json::json!({
            "name": comm.name,
            "email": comm.email,
            "date": comm.date,
        });
    }

    let res = client.post(&url).json(&body).send().context("Failed to create commit on GitHub")?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("Failed to create commit on GitHub: {}", err);
    }

    let json: serde_json::Value = res.json()?;
    let sha = json["sha"].as_str().context("Missing sha in commit response")?.to_string();
    Ok(sha)
}

/// Updates an existing branch reference on GitHub to point to a new commit SHA.
// Used by `push`.
pub fn update_github_ref(client: &reqwest::blocking::Client, target: &GitHubTarget, branch: &str, new_sha: &str, force: bool) -> Result<()> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/git/refs/heads/{}",
        target.owner, target.repo, branch
    );
    let body = serde_json::json!({
        "sha": new_sha,
        "force": force
    });

    let res = client.patch(&url).json(&body).send().context("Failed to update ref on GitHub")?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("Failed to update ref '{}' on GitHub: {}", branch, err);
    }
    Ok(())
}

/// Creates a new branch reference on GitHub pointing to a commit SHA.
// Used by `push`.
pub fn create_github_ref(client: &reqwest::blocking::Client, target: &GitHubTarget, branch: &str, sha: &str) -> Result<()> {
    let url = format!("https://api.github.com/repos/{}/{}/git/refs", target.owner, target.repo);
    let body = serde_json::json!({
        "ref": format!("refs/heads/{}", branch),
        "sha": sha
    });

    let res = client.post(&url).json(&body).send().context("Failed to create ref on GitHub")?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("Failed to create ref '{}' on GitHub: {}", branch, err);
    }
    Ok(())
}

/// Deletes a branch reference on GitHub.
// Used by `push`.
pub fn delete_github_ref(client: &reqwest::blocking::Client, target: &GitHubTarget, branch: &str) -> Result<()> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/git/refs/heads/{}",
        target.owner, target.repo, branch
    );
    let res = client.delete(&url).send().context("Failed to delete ref on GitHub")?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("Failed to delete ref '{}' on GitHub: {}", branch, err);
    }
    Ok(())
}

/// Lists all branch references from a GitHub repository, returning pairs of (branch_name, commit_sha).
// Used by `fetch`.
pub fn list_github_refs(client: &reqwest::blocking::Client, target: &GitHubTarget) -> Result<Vec<(String, String)>> {
    let url = format!("https://api.github.com/repos/{}/{}/git/refs/heads", target.owner, target.repo);
    let res = client.get(&url).send().context("Failed to list refs from GitHub")?;

    if res.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("fatal: Repository '{}/{}' not found or not accessible.", target.owner, target.repo);
    }
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        crate::remote::credential::engine::reject_pending();
        anyhow::bail!("fatal: Authentication failed for GitHub (bad credentials). Check your token (GITHUB_TOKEN, github.token or your credential helper).");
    }
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("fatal: GitHub API error listing refs: {}", err);
    }

    crate::remote::credential::engine::approve_pending();
    let json: serde_json::Value = res.json().context("Failed to parse GitHub refs response")?;
    let mut refs = Vec::new();
    if let Some(arr) = json.as_array() {
        for item in arr {
            if let (Some(ref_name), Some(sha)) = (
                item["ref"].as_str(),
                item["object"]["sha"].as_str(),
            ) {
                let branch = ref_name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(ref_name)
                    .to_string();
                refs.push((branch, sha.to_string()));
            }
        }
    }
    Ok(refs)
}

/// Gets the default branch name for a GitHub repository (usually "main" or "master").
// Used by `fetch`.
pub fn get_github_default_branch(client: &reqwest::blocking::Client, target: &GitHubTarget) -> Result<String> {
    let url = format!("https://api.github.com/repos/{}/{}", target.owner, target.repo);
    let res = client.get(&url).send().context("Failed to get repo info from GitHub")?;
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        crate::remote::credential::engine::reject_pending();
    }
    if !res.status().is_success() {
        return Ok("main".to_string());
    }
    crate::remote::credential::engine::approve_pending();
    let json: serde_json::Value = res.json().context("Failed to parse repo info")?;
    Ok(json["default_branch"]
        .as_str()
        .unwrap_or("main")
        .to_string())
}

/// Downloads a blob object from GitHub, reconstructs the raw git blob, and writes it to
/// the local `.git/objects/` store.
// Used by `fetch`.
pub fn fetch_github_blob(client: &reqwest::blocking::Client, target: &GitHubTarget, sha: &str) -> Result<()> {
    if crate::remote::fetch::engine::local_has_object(sha) {
        return Ok(());
    }

    let url = format!("https://api.github.com/repos/{}/{}/git/blobs/{}", target.owner, target.repo, sha);
    let res = client.get(&url).send().context("Failed to fetch blob from GitHub")?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("Failed to fetch blob {} from GitHub: {}", sha, err);
    }

    let json: serde_json::Value = res.json().context("Failed to parse blob response")?;
    let encoding = json["encoding"].as_str().unwrap_or("base64");
    let content_str = json["content"].as_str().context("Missing content in blob response")?;
    let content_clean: String = content_str.chars().filter(|c| !c.is_whitespace()).collect();

    let content = if encoding == "base64" {
        base64::engine::general_purpose::STANDARD.decode(&content_clean)
            .context("Failed to base64-decode blob content")?
    } else {
        content_clean.into_bytes()
    };

    crate::core::object_store::write_object("blob", &content)?;
    Ok(())
}

/// Downloads a tree object from GitHub, reconstructs the raw binary git tree, and writes
/// it to the local `.git/objects/` store. Recursively fetches all subtrees and blobs.
// Used by `fetch`.
pub fn fetch_github_tree(client: &reqwest::blocking::Client, target: &GitHubTarget, sha: &str, visited_trees: &mut std::collections::HashSet<String>) -> Result<()> {
    if !visited_trees.insert(sha.to_string()) {
        return Ok(());
    }
    if crate::remote::fetch::engine::local_has_object(sha) {
        return Ok(());
    }

    let url = format!("https://api.github.com/repos/{}/{}/git/trees/{}", target.owner, target.repo, sha);
    let res = client.get(&url).send().context("Failed to fetch tree from GitHub")?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("Failed to fetch tree {} from GitHub: {}", sha, err);
    }

    let json: serde_json::Value = res.json().context("Failed to parse tree response")?;
    let items = json["tree"].as_array().context("Missing tree array in response")?;

    for item in items {
        let item_sha = item["sha"].as_str().context("Missing sha in tree item")?.to_string();
        let item_type = item["type"].as_str().unwrap_or("blob");
        if item_type == "tree" {
            fetch_github_tree(client, target, &item_sha, visited_trees)?;
        } else {
            fetch_github_blob(client, target, &item_sha)?;
        }
    }

    let tree_content = build_tree_content(items)?;
    let written_sha = crate::core::object_store::write_object("tree", &tree_content)?;
    ensure_same_object("tree", sha, &written_sha)?;
    Ok(())
}

/// Maps a tree entry mode from the GitHub API to git's canonical form. GitHub reports
/// directories as `040000`, but git stores them as `40000`; any other spelling would give
/// the tree a different hash.
pub fn git_tree_mode(api_mode: &str) -> &str {
    if api_mode == "040000" { "40000" } else { api_mode }
}

/// Rebuilds the raw contents of a git tree from the `tree` array of a GitHub API response,
/// with git's entry order (directories sort as if their name ended in `/`).
pub fn build_tree_content(items: &[serde_json::Value]) -> Result<Vec<u8>> {
    let mut entries: Vec<(String, String, String, Vec<u8>)> = Vec::new();
    for item in items {
        let item_path = item["path"].as_str().context("Missing path in tree item")?.to_string();
        let item_sha = item["sha"].as_str().context("Missing sha in tree item")?;
        let item_mode = git_tree_mode(item["mode"].as_str().unwrap_or("100644")).to_string();
        let sort_key = if item["type"].as_str() == Some("tree") { format!("{}/", item_path) } else { item_path.clone() };
        let sha_bytes = hex::decode(item_sha).with_context(|| format!("Invalid SHA hex in tree: {}", item_sha))?;
        entries.push((sort_key, item_mode, item_path, sha_bytes));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut tree_content = Vec::new();
    for (_, mode, name, sha_bytes) in entries {
        tree_content.extend_from_slice(mode.as_bytes());
        tree_content.push(b' ');
        tree_content.extend_from_slice(name.as_bytes());
        tree_content.push(0);
        tree_content.extend_from_slice(&sha_bytes);
    }
    Ok(tree_content)
}

/// Fails when an object rebuilt from the GitHub API does not hash to the id GitHub gave.
/// The fetch then stops before any remote-tracking ref is moved, so no ref ever points at
/// an object that is missing locally.
pub fn ensure_same_object(kind: &str, expected: &str, written: &str) -> Result<()> {
    if written != expected {
        anyhow::bail!(
            "fatal: could not rebuild {} {} from the GitHub API (got {} instead).\n\
             The API does not expose every detail of the original object (for example a commit's time zone or signature).\n\
             No refs were updated. Use `git fetch` for this remote.",
            kind, expected, written
        );
    }
    Ok(())
}

/// Downloads a commit from GitHub, reconstructs the raw git commit object text, and writes
/// it to the local `.git/objects/` store. Recursively fetches its tree and parent commits.
// Used by `fetch`.
pub fn fetch_github_commit(client: &reqwest::blocking::Client, target: &GitHubTarget, sha: &str, visited_commits: &mut std::collections::HashSet<String>, visited_trees: &mut std::collections::HashSet<String>) -> Result<()> {
    if !visited_commits.insert(sha.to_string()) {
        return Ok(());
    }
    if crate::remote::fetch::engine::local_has_object(sha) {
        return Ok(());
    }

    let url = format!("https://api.github.com/repos/{}/{}/git/commits/{}", target.owner, target.repo, sha);
    let res = client.get(&url).send().context("Failed to fetch commit from GitHub")?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("Failed to fetch commit {} from GitHub: {}", sha, err);
    }

    let json: serde_json::Value = res.json().context("Failed to parse commit response")?;
    let tree_sha = json["tree"]["sha"].as_str().context("Missing tree sha in commit")?.to_string();
    fetch_github_tree(client, target, &tree_sha, visited_trees)?;

    if let Some(parents) = json["parents"].as_array() {
        for parent in parents {
            let parent_sha = parent["sha"].as_str().context("Missing parent sha")?;
            fetch_github_commit(client, target, parent_sha, visited_commits, visited_trees)?;
        }
    }

    let author_name = json["author"]["name"].as_str().unwrap_or("Unknown");
    let author_email = json["author"]["email"].as_str().unwrap_or("");
    let author_date = json["author"]["date"].as_str().unwrap_or("1970-01-01T00:00:00Z");
    let committer_name = json["committer"]["name"].as_str().unwrap_or("Unknown");
    let committer_email = json["committer"]["email"].as_str().unwrap_or("");
    let committer_date = json["committer"]["date"].as_str().unwrap_or("1970-01-01T00:00:00Z");
    let message = json["message"].as_str().unwrap_or("").trim_end_matches('\n');
    let author_ts = iso8601_to_git_timestamp(author_date);
    let committer_ts = iso8601_to_git_timestamp(committer_date);

    let mut commit_text = format!("tree {}\n", tree_sha);
    if let Some(parents) = json["parents"].as_array() {
        for parent in parents {
            if let Some(p_sha) = parent["sha"].as_str() {
                commit_text.push_str(&format!("parent {}\n", p_sha));
            }
        }
    }
    commit_text.push_str(&format!("author {} <{}> {}\n", author_name, author_email, author_ts));
    commit_text.push_str(&format!("committer {} <{}> {}\n", committer_name, committer_email, committer_ts));
    commit_text.push('\n');
    commit_text.push_str(message);
    commit_text.push('\n');

    let written_sha = crate::core::object_store::write_object("commit", commit_text.as_bytes())?;
    ensure_same_object("commit", sha, &written_sha)?;
    Ok(())
}

/// Converts an ISO 8601 UTC timestamp string to a git-format timestamp string (unix seconds + offset).
// Used by `fetch`.
pub fn iso8601_to_git_timestamp(iso: &str) -> String {
    // Parse "2024-04-11T12:22:03Z" into unix seconds (best-effort without chrono)
    let clean = iso.trim_end_matches('Z').replace('T', " ");
    let parts: Vec<&str> = clean.split(|c| c == '-' || c == ':' || c == ' ').collect();
    if parts.len() < 6 {
        return "0 +0000".to_string();
    }
    let year: u64 = parts[0].parse().unwrap_or(1970);
    let month: u64 = parts[1].parse().unwrap_or(1);
    let day: u64 = parts[2].parse().unwrap_or(1);
    let hour: u64 = parts[3].parse().unwrap_or(0);
    let minute: u64 = parts[4].parse().unwrap_or(0);
    let second: u64 = parts[5].parse().unwrap_or(0);

    // Days since Unix epoch (1970-01-01)
    let days = days_since_epoch(year, month, day);
    let secs = days * 86400 + hour * 3600 + minute * 60 + second;
    format!("{} +0000", secs)
}

/// Computes days elapsed since 1970-01-01 for a given calendar date.
// Used by `fetch`.
fn days_since_epoch(year: u64, month: u64, day: u64) -> u64 {
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let yoe = y % 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Orchestrates fetching all refs and objects from a GitHub repository via the Git Data API,
/// updating remote-tracking refs and writing FETCH_HEAD.
// Used by `fetch`.
pub fn fetch_from_github(target: &GitHubTarget, remote_name: &str, remote_display: &str, branch_filter: Option<&str>) -> Result<Vec<crate::remote::fetch::types::FetchRefResult>> {
    let client = build_github_client(&target.token)?;
    let default_branch = get_github_default_branch(&client, target)?;

    let remote_refs = list_github_refs(&client, target)?;
    let filtered_refs: Vec<_> = match branch_filter {
        Some(b) => remote_refs.into_iter().filter(|(name, _)| name == b).collect(),
        None => remote_refs,
    };

    if filtered_refs.is_empty() {
        println!("From {}", remote_display);
        println!(" * (no branches found or empty repository)");
        return Ok(Vec::new());
    }

    println!("From {}", remote_display);

    let mut results = Vec::new();
    let mut visited_commits = std::collections::HashSet::new();
    let mut visited_trees = std::collections::HashSet::new();
    let mut fetch_head_entries = Vec::new();
    let mut total_objects = 0usize;

    for (branch, remote_sha) in &filtered_refs {
        let local_sha = crate::remote::fetch::engine::read_local_remote_tracking_ref(remote_name, branch);

        if local_sha.as_deref() == Some(remote_sha.as_str()) {
            print!(" = [up to date]      {:20} -> {}/{}", branch, remote_name, branch);
            println!();
            results.push(crate::remote::fetch::types::FetchRefResult::UpToDate { branch: branch.clone() });
            fetch_head_entries.push((remote_sha.clone(), branch.clone(), remote_display.to_string()));
            continue;
        }

        let before = visited_commits.len();
        fetch_github_commit(&client, target, remote_sha, &mut visited_commits, &mut visited_trees)?;
        let fetched = visited_commits.len() - before;
        total_objects += fetched;

        crate::remote::fetch::engine::write_local_remote_tracking_ref(remote_name, branch, remote_sha)?;
        fetch_head_entries.push((remote_sha.clone(), branch.clone(), remote_display.to_string()));

        let result = match &local_sha {
            None => {
                println!(" * [new branch]      {:20} -> {}/{}", branch, remote_name, branch);
                crate::remote::fetch::types::FetchRefResult::NewBranch {
                    branch: branch.clone(),
                    sha: remote_sha.clone(),
                }
            }
            Some(old) => {
                println!(
                    "   {}..{}  {:16} -> {}/{}",
                    crate::core::commit::short_hash(old),
                    crate::core::commit::short_hash(remote_sha),
                    branch, remote_name, branch
                );
                crate::remote::fetch::types::FetchRefResult::Updated {
                    branch: branch.clone(),
                    old_sha: old.clone(),
                    new_sha: remote_sha.clone(),
                }
            }
        };
        results.push(result);
    }

    if total_objects > 0 {
        println!("Receiving objects: 100% ({}/{}), done.", total_objects, total_objects);
    }

    if let Some((_, sha)) = filtered_refs.iter().find(|(b, _)| b == &default_branch) {
        let _ = crate::core::refs::write_remote_head(remote_name, &default_branch, sha);
    }

    crate::remote::fetch::engine::write_fetch_head(&fetch_head_entries)?;

    Ok(results)
}

/// Orchestrates pushing local commits and tree/blob objects to GitHub via the Git Data API.
// Used by `push`.

pub fn push_to_github(target: &GitHubTarget, remote_name: &str, remote_display: &str, src_name: &str, dest_branch: &str, local_commit_hash: Option<&str>, force: bool,) -> Result<()> {
    let client = build_github_client(&target.token)?;

    if src_name.is_empty() {
        delete_github_ref(&client, target, dest_branch)?;
        let _ = crate::remote::push::engine::delete_remote_tracking_ref(remote_name, dest_branch);
        println!("To {}", remote_display);
        println!(" - [deleted]         {}", dest_branch);
        return Ok(());
    }

    let local_hash = local_commit_hash
        .ok_or_else(|| anyhow::anyhow!("Missing local commit hash for push"))?;

    let remote_commit_opt = get_github_branch_commit(&client, target, dest_branch)?;

    let status = match remote_commit_opt {
        Some(ref remote_hash) => {
            if remote_hash == local_hash {
                println!("Everything up-to-date");
                return Ok(());
            }

            let is_ff = crate::core::commit::is_reachable(local_hash, remote_hash).unwrap_or(false);
            if !is_ff && !force {
                eprintln!("To {}", remote_display);
                if crate::core::color::stderr_enabled("push") {
                    let red = crate::core::color::slot("push", "error", crate::core::color::RED);
                    eprintln!(" {}! [rejected]       {} {} -> {} (non-fast-forward)", red, crate::core::color::RESET, src_name, dest_branch);
                } else {
                    eprintln!(" ! [rejected]        {} -> {} (non-fast-forward)", src_name, dest_branch);
                }
                anyhow::bail!(
                    "error: failed to push some refs to '{}'\n\
                     hint: Updates were rejected because the tip of your current branch is behind\n\
                     hint: its remote counterpart. Integrate the remote changes (e.g.\n\
                     hint: 'rgit merge') before pushing again.",
                    remote_display
                );
            }

            if is_ff {
                PushStatus::FastForward {
                    old_hash: remote_hash.clone(),
                    new_hash: local_hash.to_string(),
                }
            } else {
                PushStatus::ForcedUpdate {
                    old_hash: remote_hash.clone(),
                    new_hash: local_hash.to_string(),
                }
            }
        }
        None => PushStatus::NewBranch {
            new_hash: local_hash.to_string(),
        },
    };

    let stop_hash = match &status {
        PushStatus::FastForward { old_hash, .. } => Some(old_hash.as_str()),
        _ => None,
    };

    let mut missing_commits = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(local_hash.to_string());

    while let Some(hash) = queue.pop_front() {
        if !visited.insert(hash.clone()) {
            continue;
        }
        if Some(hash.as_str()) == stop_hash {
            continue;
        }
        missing_commits.push(hash.clone());

        let (obj_type, content) = crate::core::object_store::read_object(&hash)?;
        if obj_type == "commit" {
            let text = String::from_utf8_lossy(&content);
            for line in text.lines() {
                if line.is_empty() {
                    break;
                }
                if let Some(parent) = line.strip_prefix("parent ") {
                    if Some(parent) != stop_hash {
                        queue.push_back(parent.to_string());
                    }
                }
            }
        }
    }
    missing_commits.reverse();

    println!("Enumerating objects: {}, done.", missing_commits.len());

    let mut uploaded_blobs = HashSet::new();
    let mut uploaded_trees = HashMap::new();
    // Everything under the remote branch's current tree is already on GitHub with the same
    // hashes, so only files and folders that changed since then get uploaded.
    if let Some(remote_hash) = &remote_commit_opt
        && let Ok(remote_tree) = crate::core::object_store::tree_hash_of_commit(remote_hash)
    {
        let read = |hash: &str| crate::core::object_store::read_object(hash);
        // A partial walk is still correct (it only lists objects GitHub has); it just saves less.
        let _ = collect_known_tree_objects(&remote_tree, &read, &mut uploaded_trees, &mut uploaded_blobs);
    }
    let mut commit_map: HashMap<String, String> = HashMap::new();

    let mut count = 0;
    for commit_hash in &missing_commits {
        let (obj_type, content) = crate::core::object_store::read_object(commit_hash)?;
        if obj_type != "commit" {
            anyhow::bail!("{} is not a commit object", commit_hash);
        }

        let text = String::from_utf8_lossy(&content);
        let mut tree_hash = String::new();
        let mut parents = Vec::new();
        let mut author = None;
        let mut committer = None;

        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(t) = line.strip_prefix("tree ") {
                tree_hash = t.to_string();
            } else if let Some(p) = line.strip_prefix("parent ") {
                parents.push(p.to_string());
            } else if line.starts_with("author ") {
                author = parse_commit_identity(line);
            } else if line.starts_with("committer ") {
                committer = parse_commit_identity(line);
            }
        }

        let message = raw_commit_message(&text);
        let remote_tree_sha = upload_tree_to_github(
            &client,
            target,
            &tree_hash,
            &mut uploaded_blobs,
            &mut uploaded_trees,
        )?;

        let mapped_parents: Vec<String> = parents
            .iter()
            .map(|p| commit_map.get(p).cloned().unwrap_or_else(|| p.clone()))
            .collect();

        let remote_commit_sha = create_github_commit(
            &client,
            target,
            message,
            &remote_tree_sha,
            &mapped_parents,
            author.as_ref(),
            committer.as_ref(),
        )?;

        ensure_pushed_commit_matches(commit_hash, &remote_commit_sha, dest_branch)?;
        commit_map.insert(commit_hash.clone(), remote_commit_sha);
        count += 1;
    }

    if count > 0 {
        println!("Writing objects: 100% ({}/{}), done.", count, missing_commits.len());
        println!("Total {} (delta 0), reused 0 (delta 0)", count);
    }

    let final_remote_sha = commit_map
        .get(local_hash)
        .cloned()
        .unwrap_or_else(|| local_hash.to_string());

    match status {
        PushStatus::FastForward { .. } | PushStatus::ForcedUpdate { .. } => {
            update_github_ref(&client, target, dest_branch, &final_remote_sha, force)?;
        }
        PushStatus::NewBranch { .. } => {
            create_github_ref(&client, target, dest_branch, &final_remote_sha)?;
        }
    }

    let _ = crate::remote::push::engine::write_remote_tracking_ref(
        remote_name,
        dest_branch,
        &final_remote_sha,
    );

    println!("To {}", remote_display);
    match status {
        PushStatus::FastForward { ref old_hash, .. } => {
            println!(
                "   {}..{}  {} -> {}",
                crate::core::commit::short_hash(old_hash),
                crate::core::commit::short_hash(&final_remote_sha),
                src_name,
                dest_branch
            );
        }
        PushStatus::ForcedUpdate { ref old_hash, .. } => {
            println!(
                " + {}...{} {} -> {} (forced update)",
                crate::core::commit::short_hash(old_hash),
                crate::core::commit::short_hash(&final_remote_sha),
                src_name,
                dest_branch
            );
        }
        PushStatus::NewBranch { .. } => {
            println!(" * [new branch]      {} -> {}", src_name, dest_branch);
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/remote/github.rs"]
mod tests;

