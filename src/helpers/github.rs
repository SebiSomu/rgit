use anyhow::{Context, Result};
use base64::Engine;
use std::collections::{HashMap, HashSet, VecDeque};
use crate::objects::{CommitIdentity, GitHubTarget, GitHubTreeEntry, PushStatus};

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
    let timestamp_str = rest.split_whitespace().next()?;
    let secs: u64 = timestamp_str.parse().unwrap_or(0);
    let date = format_unix_timestamp_iso8601(secs);

    Some(CommitIdentity { name, email, date })
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

/// Retrieves the GitHub personal access token from environment variables or git configuration.
// Used by `push`.
pub fn resolve_github_token(remote_name: &str) -> Result<String> {
    if let Ok(tok) = std::env::var("GITHUB_TOKEN") {
        if !tok.trim().is_empty() {
            return Ok(tok.trim().to_string());
        }
    }

    if let Ok(tok) = std::env::var("RGIT_GITHUB_TOKEN") {
        if !tok.trim().is_empty() {
            return Ok(tok.trim().to_string());
        }
    }

    if let Some(tok) = crate::helpers::config::get_config_value(&format!("remote.{}.token", remote_name))? {
        if !tok.trim().is_empty() {
            return Ok(tok.trim().to_string());
        }
    }

    if let Some(tok) = crate::helpers::config::get_config_value("github.token")? {
        if !tok.trim().is_empty() {
            return Ok(tok.trim().to_string());
        }
    }

    anyhow::bail!(
        "fatal: GitHub token not found.\n\
         Please provide a Personal Access Token with repo scope via:\n\
         - Setting environment variable GITHUB_TOKEN\n\
         - Or configuring via: rgit config github.token <your-token>"
    )
}

/// Constructs a preconfigured blocking HTTP client with standard GitHub API headers.
// Used by `push`.
pub fn build_github_client(token: &str) -> Result<reqwest::blocking::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static("rgit/0.1.0"),
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
        anyhow::bail!("fatal: Authentication failed for GitHub (bad credentials). Check your GITHUB_TOKEN.");
    }
    if res.status() == reqwest::StatusCode::FORBIDDEN {
        anyhow::bail!("fatal: GitHub API access forbidden or rate limit exceeded for repo '{}/{}'.", target.owner, target.repo);
    }
    if !res.status().is_success() {
        let err_body = res.text().unwrap_or_default();
        anyhow::bail!("fatal: GitHub API error ({}): {}", url, err_body);
    }

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
    let (obj_type, content) = crate::helpers::read_object(blob_hash)?;
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

/// Recursively traverses and uploads a local tree object and its subtrees/blobs to GitHub.
// Used by `push`.
pub fn upload_tree_to_github(client: &reqwest::blocking::Client, target: &GitHubTarget, tree_hash: &str, uploaded_blobs: &mut HashSet<String>, uploaded_trees: &mut HashMap<String, String>) -> Result<String> {
    if let Some(cached_sha) = uploaded_trees.get(tree_hash) {
        return Ok(cached_sha.clone());
    }

    let (obj_type, content) = crate::helpers::read_object(tree_hash)?;
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
        anyhow::bail!("fatal: Authentication failed for GitHub (bad credentials). Check your GITHUB_TOKEN.");
    }
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        anyhow::bail!("fatal: GitHub API error listing refs: {}", err);
    }

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
    if !res.status().is_success() {
        return Ok("main".to_string());
    }
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
    if crate::helpers::fetch::local_has_object(sha) {
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

    crate::helpers::write_object("blob", &content)?;
    Ok(())
}

/// Downloads a tree object from GitHub, reconstructs the raw binary git tree, and writes
/// it to the local `.git/objects/` store. Recursively fetches all subtrees and blobs.
// Used by `fetch`.
pub fn fetch_github_tree(client: &reqwest::blocking::Client, target: &GitHubTarget, sha: &str, visited_trees: &mut std::collections::HashSet<String>) -> Result<()> {
    if !visited_trees.insert(sha.to_string()) {
        return Ok(());
    }
    if crate::helpers::fetch::local_has_object(sha) {
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

    let mut entries: Vec<(String, String, String, Vec<u8>)> = Vec::new();
    for item in items {
        let item_path = item["path"].as_str().context("Missing path in tree item")?.to_string();
        let item_sha = item["sha"].as_str().context("Missing sha in tree item")?.to_string();
        let item_mode = item["mode"].as_str().unwrap_or("100644").to_string();
        let item_type = item["type"].as_str().unwrap_or("blob");

        let sort_key = if item_type == "tree" {
            format!("{}/", item_path)
        } else {
            item_path.clone()
        };

        let sha_bytes = hex::decode(&item_sha)
            .with_context(|| format!("Invalid SHA hex in tree: {}", item_sha))?;

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

    let written_sha = crate::helpers::write_object("tree", &tree_content)?;
    if written_sha != sha {
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
    if crate::helpers::fetch::local_has_object(sha) {
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

    crate::helpers::write_object("commit", commit_text.as_bytes())?;
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
pub fn fetch_from_github(target: &GitHubTarget, remote_name: &str, remote_display: &str, branch_filter: Option<&str>) -> Result<Vec<crate::objects::FetchRefResult>> {
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
        let local_sha = crate::helpers::fetch::read_local_remote_tracking_ref(remote_name, branch);

        if local_sha.as_deref() == Some(remote_sha.as_str()) {
            print!(" = [up to date]      {:20} -> {}/{}", branch, remote_name, branch);
            println!();
            results.push(crate::objects::FetchRefResult::UpToDate { branch: branch.clone() });
            fetch_head_entries.push((remote_sha.clone(), branch.clone(), remote_display.to_string()));
            continue;
        }

        let before = visited_commits.len();
        fetch_github_commit(&client, target, remote_sha, &mut visited_commits, &mut visited_trees)?;
        let fetched = visited_commits.len() - before;
        total_objects += fetched;

        crate::helpers::fetch::write_local_remote_tracking_ref(remote_name, branch, remote_sha)?;
        fetch_head_entries.push((remote_sha.clone(), branch.clone(), remote_display.to_string()));

        let result = match &local_sha {
            None => {
                println!(" * [new branch]      {:20} -> {}/{}", branch, remote_name, branch);
                crate::objects::FetchRefResult::NewBranch {
                    branch: branch.clone(),
                    sha: remote_sha.clone(),
                }
            }
            Some(old) => {
                println!(
                    "   {}..{}  {:16} -> {}/{}",
                    crate::helpers::short_hash(old),
                    crate::helpers::short_hash(remote_sha),
                    branch, remote_name, branch
                );
                crate::objects::FetchRefResult::Updated {
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
        let _ = crate::refs::write_ref(&format!("refs/remotes/{}/HEAD", remote_name), sha);
    }

    crate::helpers::fetch::write_fetch_head(&fetch_head_entries)?;

    Ok(results)
}

/// Orchestrates pushing local commits and tree/blob objects to GitHub via the Git Data API.
// Used by `push`.

pub fn push_to_github(target: &GitHubTarget, remote_name: &str, remote_display: &str, src_name: &str, dest_branch: &str, local_commit_hash: Option<&str>, force: bool,) -> Result<()> {
    let client = build_github_client(&target.token)?;

    if src_name.is_empty() {
        delete_github_ref(&client, target, dest_branch)?;
        let _ = crate::helpers::push::delete_remote_tracking_ref(remote_name, dest_branch);
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

            let is_ff = crate::helpers::is_reachable(local_hash, remote_hash).unwrap_or(false);
            if !is_ff && !force {
                eprintln!("To {}", remote_display);
                eprintln!(" ! [rejected]        {} -> {} (non-fast-forward)", src_name, dest_branch);
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

        let (obj_type, content) = crate::helpers::read_object(&hash)?;
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
    let mut commit_map: HashMap<String, String> = HashMap::new();

    let mut count = 0;
    for commit_hash in &missing_commits {
        let (obj_type, content) = crate::helpers::read_object(commit_hash)?;
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

        let message = crate::helpers::extract_commit_message(&text);
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
            &message,
            &remote_tree_sha,
            &mapped_parents,
            author.as_ref(),
            committer.as_ref(),
        )?;

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
        _ => {}
    }

    let _ = crate::helpers::push::write_remote_tracking_ref(
        remote_name,
        dest_branch,
        &final_remote_sha,
    );

    println!("To {}", remote_display);
    match status {
        PushStatus::FastForward { ref old_hash, .. } => {
            println!(
                "   {}..{}  {} -> {}",
                crate::helpers::short_hash(old_hash),
                crate::helpers::short_hash(&final_remote_sha),
                src_name,
                dest_branch
            );
        }
        PushStatus::ForcedUpdate { ref old_hash, .. } => {
            println!(
                " + {}...{} {} -> {} (forced update)",
                crate::helpers::short_hash(old_hash),
                crate::helpers::short_hash(&final_remote_sha),
                src_name,
                dest_branch
            );
        }
        PushStatus::NewBranch { .. } => {
            println!(" * [new branch]      {} -> {}", src_name, dest_branch);
        }
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_unix_timestamp_iso8601() {
        assert_eq!(format_unix_timestamp_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix_timestamp_iso8601(1712838123), "2024-04-11T12:22:03Z");
        assert_eq!(format_unix_timestamp_iso8601(1775898000), "2026-04-11T09:00:00Z");
    }

    #[test]
    fn test_parse_commit_identity() {
        let line = "author Linus Torvalds <torvalds@linux-foundation.org> 1712838123 +0000";
        let ident = parse_commit_identity(line).expect("Should parse author line");
        assert_eq!(ident.name, "Linus Torvalds");
        assert_eq!(ident.email, "torvalds@linux-foundation.org");
        assert_eq!(ident.date, "2024-04-11T12:22:03Z");

        let committer_line = "committer Octocat <octo@github.com> 0 +0200";
        let committer = parse_commit_identity(committer_line).expect("Should parse committer");
        assert_eq!(committer.name, "Octocat");
        assert_eq!(committer.email, "octo@github.com");
        assert_eq!(committer.date, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_parse_github_remote() {
        assert_eq!(
            parse_github_remote("https://github.com/SebiSomu/RGit.git"),
            Some(("SebiSomu".to_string(), "RGit".to_string()))
        );
        assert_eq!(
            parse_github_remote("https://github.com/owner/my-repo"),
            Some(("owner".to_string(), "my-repo".to_string()))
        );
        assert_eq!(
            parse_github_remote("git@github.com:rust-lang/cargo.git"),
            Some(("rust-lang".to_string(), "cargo".to_string()))
        );
        assert_eq!(
            parse_github_remote("git@github.com:owner/repo"),
            Some(("owner".to_string(), "repo".to_string()))
        );
        assert_eq!(
            parse_github_remote("github.com/torvalds/linux"),
            Some(("torvalds".to_string(), "linux".to_string()))
        );
        assert_eq!(
            parse_github_remote("nonexistent_owner/nonexistent_repo"),
            Some(("nonexistent_owner".to_string(), "nonexistent_repo".to_string()))
        );
        assert_eq!(parse_github_remote("./local/dir"), None);
        assert_eq!(parse_github_remote("local_repo"), None);
    }

    #[test]
    fn test_iso8601_to_git_timestamp() {
        assert_eq!(iso8601_to_git_timestamp("1970-01-01T00:00:00Z"), "0 +0000");
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(iso8601_to_git_timestamp("2024-01-01T00:00:00Z"), "1704067200 +0000");
    }
}
