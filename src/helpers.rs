use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use anyhow::{Context, Result};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use crate::index::{read_index, write_index, build_entry, IndexEntry};
use crate::refs;
use crate::objects::*;

/// Hashes content, writes the zlib-compressed object to `.git/objects/`,
/// and returns the hexadecimal SHA-1 string.
// Used by commands that write objects (like `hash-object`, `write-tree`, `commit`, and staging index updates).
pub fn write_object(object_type: &str, content: &[u8]) -> Result<String> {
    let header = format!("{} {}\0", object_type, content.len());

    let mut store = header.into_bytes();
    store.extend_from_slice(content);

    let mut hasher = Sha1::new();
    hasher.update(&store);

    let hash = hasher.finalize();
    let hash_hex = hex::encode(hash);

    let dir = format!(".git/objects/{}", &hash_hex[..2]);
    let path = format!("{}/{}", dir, &hash_hex[2..]);

    fs::create_dir_all(&dir)?;

    if !Path::new(&path).exists() {
        let file = fs::File::create(&path)?;
        let mut encoder = ZlibEncoder::new(file, Compression::default());

        encoder.write_all(&store)?;
        encoder.finish()?;
    }

    Ok(hash_hex)
}

/// Reads a zlib-compressed object from `.git/objects/` and returns its
/// object type and decompressed content.
// Used by commands that read objects (like `cat-file`, `log`, `status`, `switch`, `checkout`, `restore`, and merge checks).
pub fn read_object(hash: &str) -> Result<(String, Vec<u8>)> {
    if hash.len() < 3 {
        anyhow::bail!("Invalid object hash: {}", hash);
    }

    let dir = &hash[0..2];
    let file_name = &hash[2..];
    let path = format!(".git/objects/{}/{}", dir, file_name);
    let compressed_data = fs::read(&path).context("Failed to read object file (does this hash exist?)")?;
    let mut decoder = ZlibDecoder::new(&compressed_data[..]);
    let mut decompressed_data = Vec::new();
    decoder.read_to_end(&mut decompressed_data)?;

    let null_pos = decompressed_data
        .iter()
        .position(|&b| b == 0)
        .context("Invalid Git object format")?;

    let header = String::from_utf8_lossy(&decompressed_data[..null_pos]);
    let object_type = header
        .split(' ')
        .next()
        .context("Invalid Git object header")?
        .to_string();

    let content = decompressed_data[null_pos + 1..].to_vec();

    Ok((object_type, content))
}



/// Parses a commit object to retrieve the tree hash associated with it.
// Used by `status`, `switch`, `checkout`, and `restore` to load trees.
pub fn tree_hash_of_commit(commit_hash: &str) -> Result<String> {
    let (object_type, content) = read_object(commit_hash)?;

    if object_type != "commit" {
        anyhow::bail!("{} is not a commit object", commit_hash);
    }

    let text = String::from_utf8_lossy(&content);
    let tree_line = text
        .lines()
        .next()
        .context("Malformed commit: missing tree line")?;

    tree_line
        .strip_prefix("tree ")
        .map(|s| s.to_string())
        .context("Malformed commit: first line isn't a tree line")
}

/// Matches a string against a glob pattern supporting `*`, `?`, and `**`.
// Used by `.gitignore` pattern matching in `helpers.rs`.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pat_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();
    glob_match_slice(&pat_chars, &text_chars)
}

fn glob_match_slice(pat: &[char], text: &[char]) -> bool {
    if pat.is_empty() {
        return text.is_empty();
    }

    if pat.starts_with(&['*', '*']) {
        let mut rest_pat = &pat[2..];
        if rest_pat.starts_with(&['/']) {
            rest_pat = &rest_pat[1..];
        }
        for i in 0..=text.len() {
            if glob_match_slice(rest_pat, &text[i..]) {
                return true;
            }
        }
        return false;
    }

    if pat[0] == '*' {
        let rest_pat = &pat[1..];
        let mut i = 0;
        while i <= text.len() {
            if glob_match_slice(rest_pat, &text[i..]) {
                return true;
            }
            if i < text.len() && text[i] == '/' {
                break;
            }
            i += 1;
        }
        return false;
    }

    if text.is_empty() {
        return false;
    }

    if pat[0] == '?' {
        if text[0] != '/' {
            return glob_match_slice(&pat[1..], &text[1..]);
        } else {
            return false;
        }
    }

    if pat[0] == text[0] {
        return glob_match_slice(&pat[1..], &text[1..]);
    }

    false
}

/// Parses a single line from a `.gitignore` file into a `GitIgnoreRule`.
// Used by `.gitignore` loading in `collect_files`.
pub fn parse_gitignore_line(line: &str, base_dir: &str) -> Option<GitIgnoreRule> {
    let mut trimmed = line.trim_end_matches(['\r', '\n']).trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let negated = if trimmed.starts_with('!') {
        trimmed = &trimmed[1..];
        true
    } else {
        false
    };

    let dir_only = if trimmed.ends_with('/') {
        trimmed = &trimmed[..trimmed.len() - 1];
        true
    } else {
        false
    };

    let (pattern, has_slash) = if trimmed.starts_with('/') {
        (&trimmed[1..], true)
    } else if trimmed.contains('/') {
        (trimmed, true)
    } else {
        (trimmed, false)
    };

    Some(GitIgnoreRule {
        pattern: pattern.to_string(),
        base_dir: base_dir.to_string(),
        negated,
        dir_only,
        has_slash,
    })
}

/// Checks whether a given relative path matches any active `.gitignore` rules.
// Used by `collect_files` and `status` checks.
pub fn is_path_ignored(rules: &[GitIgnoreRule], rel_path: &str, is_dir: bool) -> bool {
    let mut ignored = false;

    for rule in rules {
        if rule.dir_only && !is_dir {
            continue;
        }

        let target_subpath = if rule.base_dir.is_empty() {
            rel_path
        } else {
            if rel_path == rule.base_dir {
                ""
            } else if rel_path.starts_with(&format!("{}/", rule.base_dir)) {
                &rel_path[rule.base_dir.len() + 1..]
            } else {
                continue;
            }
        };

        let matched = if rule.has_slash {
            glob_match(&rule.pattern, target_subpath)
        } else {
            let filename = Path::new(rel_path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if glob_match(&rule.pattern, &filename) {
                true
            } else {
                target_subpath
                    .split('/')
                    .any(|part| glob_match(&rule.pattern, part))
            }
        };

        if matched {
            ignored = !rule.negated;
        }
    }

    ignored
}

/// Recursively collects all files under the given path, respecting `.gitignore` rules
/// and ignoring the `.git/` folder.
// Used by index staging (`add`), status, and working tree safety checks.
pub fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut rules = Vec::new();
    collect_files_internal(path, &mut rules, out)
}

fn collect_files_internal(path: &Path, rules: &mut Vec<GitIgnoreRule>, out: &mut Vec<PathBuf>) -> Result<()> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("path '{}' did not match any files", path.display()))?;

    let rel_path = normalize_path(path);

    if metadata.is_file() {
        if !rel_path.is_empty() && is_path_ignored(rules, &rel_path, false) {
            return Ok(());
        }
        out.push(path.to_path_buf());
        return Ok(());
    }

    if metadata.is_dir() {
        let name_str = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if name_str == ".git" {
            return Ok(());
        }

        if !rel_path.is_empty() && is_path_ignored(rules, &rel_path, true) {
            return Ok(());
        }

        let mut current_rules = rules.clone();
        let gitignore_file = path.join(".gitignore");
        if gitignore_file.exists() && gitignore_file.is_file() {
            if let Ok(content) = fs::read_to_string(&gitignore_file) {
                for line in content.lines() {
                    if let Some(rule) = parse_gitignore_line(line, &rel_path) {
                        current_rules.push(rule);
                    }
                }
            }
        }

        let mut dir_entries: Vec<_> = fs::read_dir(path)?.filter_map(Result::ok).collect();
        dir_entries.sort_by_key(|e| e.file_name());

        for entry in dir_entries {
            let entry_name = entry.file_name().to_string_lossy().to_string();
            if entry_name == ".git" {
                continue;
            }
            collect_files_internal(&entry.path(), &mut current_rules, out)?;
        }
    }

    Ok(())
}

/// Normalizes a file path into a relative, forward-slash separated path string.
// Used for mapping paths consistently across platform boundaries in the index and object trees.
pub fn normalize_path(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Calculates the 20-byte SHA-1 hash for the given content type and bytes.
// Used by index staging and status checks to match files without writing them.
pub fn hash_content(obj_type: &str, content: &[u8]) -> [u8; 20] {
    let header = format!("{} {}\0", obj_type, content.len());
    let mut store = header.into_bytes();
    store.extend_from_slice(content);
    let mut hasher = Sha1::new();
    hasher.update(&store);
    hasher.finalize().into()
}

/// Recursively flattens tree objects starting from `tree_hash` into a map
/// of relative paths to their corresponding hashes and modes.
// Used by `status`, `switch`, `checkout`, and `restore` commands.
pub fn flatten_tree(tree_hash: &str, prefix: &str, out: &mut BTreeMap<String, ([u8; 20], u32)>) -> Result<()> {
    let (object_type, content) = read_object(tree_hash)?;
    if object_type != "tree" {
        anyhow::bail!("{} is not a tree object", tree_hash);
    }

    let mut pos = 0;
    while pos < content.len() {
        let space_pos = content[pos..].iter().position(|&b| b == b' ').context("Missing mode separator")? + pos;
        let mode_str = String::from_utf8_lossy(&content[pos..space_pos]).to_string();

        let null_pos = content[space_pos..].iter().position(|&b| b == 0).context("Missing name terminator")? + space_pos;
        let name = String::from_utf8_lossy(&content[space_pos + 1..null_pos]).to_string();

        let hash_start = null_pos + 1;
        let hash_end = hash_start + 20;
        let mut hash = [0u8; 20];
        hash.copy_from_slice(&content[hash_start..hash_end]);

        let full_path = if prefix.is_empty() { name } else { format!("{}/{}", prefix, name) };

        if mode_str == "40000" {
            flatten_tree(&hex::encode(hash), &full_path, out)?;
        } else {
            let mode = u32::from_str_radix(&mode_str, 8).unwrap_or(0o100644);
            out.insert(full_path, (hash, mode));
        }

        pos = hash_end;
    }

    Ok(())
}

/// Checks whether switching branches would overwrite untracked or modified files in the working directory.
// Used by `switch` and `checkout` commands to prevent data loss.
pub fn check_switch_safety(target_tree: &BTreeMap<String, ([u8; 20], u32)>, head_tree: &BTreeMap<String, ([u8; 20], u32)>, index_map: &BTreeMap<String, [u8; 20]>) -> Result<()> {
    let mut working_files = Vec::new();
    collect_files(Path::new("."), &mut working_files)?;
    let mut working_map = BTreeMap::new();
    for file_path in &working_files {
        let rel_path = normalize_path(file_path);
        let content = fs::read(file_path)?;
        working_map.insert(rel_path, hash_content("blob", &content));
    }

    let mut local_changes = std::collections::BTreeSet::new();

    for (path, work_hash) in &working_map {
        match index_map.get(path) {
            None => {
                if let Some((target_hash, _)) = target_tree.get(path) {
                    if target_hash != work_hash {
                        anyhow::bail!("error: The following untracked working tree files would be overwritten by switch:\n\t{}\nPlease move or remove them before you switch branches.", path);
                    }
                }
            }
            Some(idx_hash) => {
                if idx_hash != work_hash {
                    local_changes.insert(path.clone());
                }
            }
        }
    }
    for path in index_map.keys() {
        if !working_map.contains_key(path) {
            local_changes.insert(path.clone());
        }
    }

    for (path, idx_hash) in index_map {
        match head_tree.get(path) {
            None => {
                local_changes.insert(path.clone());
            }
            Some((head_hash, _)) => {
                if head_hash != idx_hash {
                    local_changes.insert(path.clone());
                }
            }
        }
    }
    for path in head_tree.keys() {
        if !index_map.contains_key(path) {
            local_changes.insert(path.clone());
        }
    }

    let mut overwritten_files: Vec<String> = Vec::new();
    for path in &local_changes {
        let file_changes_in_switch = match (head_tree.get(path), target_tree.get(path)) {
            (Some((h, _)), Some((t, _))) => h != t,
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (None, None) => false,
        };

        if !file_changes_in_switch {
            continue;
        }
        overwritten_files.push(path.clone());
    }

    if !overwritten_files.is_empty() {
        let mut msg = String::from("error: Your local changes to the following files would be overwritten by switch:\n");
        for file in overwritten_files {
            msg.push_str(&format!("\t{}\n", file));
        }
        msg.push_str("Please commit your changes or stash them before you switch branches.\nAborting");
        anyhow::bail!(msg);
    }

    Ok(())
}

/// Synchronizes the files in the working directory to match the target commit tree.
/// Writes modified/new files and cleans up files removed in the target.
// Used by `switch` and `checkout` commands.
pub fn sync_working_tree(target_tree: &BTreeMap<String, ([u8; 20], u32)>, head_tree: &BTreeMap<String, ([u8; 20], u32)>, index_map: &BTreeMap<String, [u8; 20]>) -> Result<()> {
    let mut files_to_delete = std::collections::BTreeSet::new();
    for path in index_map.keys() {
        if !target_tree.contains_key(path) {
            files_to_delete.insert(path.clone());
        }
    }
    for path in head_tree.keys() {
        if !target_tree.contains_key(path) {
            files_to_delete.insert(path.clone());
        }
    }

    for path in files_to_delete {
        let path_obj = Path::new(&path);
        if path_obj.exists() {
            fs::remove_file(path_obj)?;
            let mut parent = path_obj.parent();
            while let Some(p) = parent {
                if p == Path::new("") || p == Path::new(".") {
                    break;
                }
                if p.exists() {
                    if fs::read_dir(p)?.next().is_none() {
                        fs::remove_dir(p)?;
                    } else {
                        break;
                    }
                }
                parent = p.parent();
            }
        }
    }

    for (path, (hash, _mode)) in target_tree {
        let should_write = match head_tree.get(path) {
            None => true,                          
            Some((head_hash, _)) => head_hash != hash, 
        };

        if should_write {
            let (_, content) = read_object(&hex::encode(hash))?;
            if let Some(parent) = Path::new(&path).parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, &content)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let file_mode = if (_mode & 0o111) != 0 { 0o755 } else { 0o644 };
                fs::set_permissions(path, fs::Permissions::from_mode(file_mode))?;
            }
        }
    }

    Ok(())
}

/// Updates the index entries to reflect target branch files after a switch.
// Used by `switch` and `checkout` commands.
pub fn update_index_from_tree(target_tree: &BTreeMap<String, ([u8; 20], u32)>, head_tree: &BTreeMap<String, ([u8; 20], u32)>) -> Result<()> {
    let mut entries = read_index().unwrap_or_default();
    entries.retain(|e| target_tree.contains_key(&e.path));

    for (path, (hash, _)) in target_tree {
        let should_update = match head_tree.get(path) {
            None => true,
            Some((head_hash, _)) => head_hash != hash,
        };

        if should_update {
            let metadata = fs::metadata(path)?;
            let new_entry = build_entry(path, *hash, &metadata);
            entries.retain(|e| &e.path != path);
            entries.push(new_entry);
        }
    }

    write_index(&mut entries)?;
    Ok(())
}

/// Traverses parent commit hashes via BFS to check if the target commit is reachable
/// from the start commit.
// Used by `branch -d` safety checks to verify merge status.
pub fn is_reachable(start: &str, target: &str) -> Result<bool> {
    use std::collections::{HashSet, VecDeque};

    if start == target {
        return Ok(true);
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    queue.push_back(start.to_string());

    while let Some(hash) = queue.pop_front() {
        if visited.contains(&hash) {
            continue;
        }
        visited.insert(hash.clone());

        if hash == target {
            return Ok(true);
        }

        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            continue;
        }

        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(parent_hash) = line.strip_prefix("parent ") {
                queue.push_back(parent_hash.to_string());
            }
        }
    }

    Ok(false)
}

/// Resolves a source branch, HEAD, commit hash, or parent expression (like `~N`)
/// into its commit hash.
// Used by `merge`, `diff`, `restore`, and tree resolution helpers.
pub fn resolve_commit_from_source(source: &str) -> Result<String> {
    let (base_name, steps) = if let Some(idx) = source.find('~') {
        let base = &source[..idx];
        let num_str = &source[idx + 1..];
        let num: usize = num_str.parse().unwrap_or(1);
        (base, num)
    } else {
        (source, 0)
    };

    let mut commit_hash = if base_name.eq_ignore_ascii_case("head") {
        refs::resolve_head_commit()?.ok_or_else(|| {
            anyhow::anyhow!("error: could not resolve HEAD: HEAD has no commits yet")
        })?
    } else {
        let branch_ref = format!("refs/heads/{}", base_name);
        if let Some(hash) = refs::read_ref(&branch_ref)? {
            hash
        } else {
            match read_object(base_name) {
                Ok((ref obj_type, _)) if obj_type == "commit" => base_name.to_string(),
                Ok((obj_type, _)) => {
                    anyhow::bail!("error: '{}' is not a commit (it is a {})", base_name, obj_type);
                }
                Err(_) => {
                    anyhow::bail!("error: invalid reference: '{}'", base_name);
                }
            }
        }
    };

    for _ in 0..steps {
        let (obj_type, content) = read_object(&commit_hash)?;
        if obj_type != "commit" {
            anyhow::bail!("error: object {} is not a commit", commit_hash);
        }
        let text = String::from_utf8_lossy(&content);
        let parent = text.lines().find_map(|line| line.strip_prefix("parent ")).map(|s| s.to_string());
        if let Some(p) = parent {
            commit_hash = p;
        } else {
            anyhow::bail!("error: commit {} has no parent", commit_hash);
        }
    }

    Ok(commit_hash)
}

/// Resolves a source branch/commit name/hash into its tree structure, returning
/// a map of paths to their object hashes and modes.
// Used by `restore`, `diff`, and `merge` commands.
pub fn resolve_tree_from_source(source: &str) -> Result<BTreeMap<String, ([u8; 20], u32)>> {
    let commit_hash = resolve_commit_from_source(source)?;
    let tree_hash = tree_hash_of_commit(&commit_hash)?;
    let mut tree_map = BTreeMap::new();
    flatten_tree(&tree_hash, "", &mut tree_map)?;
    Ok(tree_map)
}

/// Finds the best common ancestor (merge base) between two commits using BFS.
// Used by the `merge` command for 3-way merges.
pub fn find_merge_base(commit_a: &str, commit_b: &str) -> Result<Option<String>> {
    use std::collections::{HashSet, VecDeque};

    if commit_a == commit_b {
        return Ok(Some(commit_a.to_string()));
    }

    let mut ancestors_a = HashSet::new();
    let mut queue_a = VecDeque::new();
    queue_a.push_back(commit_a.to_string());

    while let Some(hash) = queue_a.pop_front() {
        if ancestors_a.contains(&hash) {
            continue;
        }
        ancestors_a.insert(hash.clone());

        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            continue;
        }

        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(parent_hash) = line.strip_prefix("parent ") {
                queue_a.push_back(parent_hash.to_string());
            }
        }
    }

    let mut visited_b = HashSet::new();
    let mut queue_b = VecDeque::new();
    queue_b.push_back(commit_b.to_string());

    while let Some(hash) = queue_b.pop_front() {
        if visited_b.contains(&hash) {
            continue;
        }
        visited_b.insert(hash.clone());

        if ancestors_a.contains(&hash) {
            return Ok(Some(hash));
        }

        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            continue;
        }

        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(parent_hash) = line.strip_prefix("parent ") {
                queue_b.push_back(parent_hash.to_string());
            }
        }
    }

    Ok(None)
}

/// Builds and writes a merge commit object containing two parent hashes.
// Used by the `merge` command when creating a 3-way merge commit.
pub fn build_merge_commit(tree_hash: String, parent1: &str, parent2: &str, message: &str) -> Result<String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut content = format!("tree {}\n", tree_hash);
    content.push_str(&format!("parent {}\n", parent1));
    content.push_str(&format!("parent {}\n", parent2));

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let author = "rgit <rgit@example.com>";

    content.push_str(&format!("author {} {} +0000\n", author, timestamp));
    content.push_str(&format!("committer {} {} +0000\n", author, timestamp));
    content.push_str(&format!("\n{}\n", message));

    write_object("commit", content.as_bytes())
}

/// Generates file content containing conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`).
// Used by the `merge` command when conflicts are detected.
pub fn generate_conflict_markers(ours_content: &[u8], theirs_content: &[u8], branch_name: &str) -> Vec<u8> {
    let ours_str = String::from_utf8_lossy(ours_content);
    let theirs_str = String::from_utf8_lossy(theirs_content);

    let mut out = String::new();
    out.push_str("<<<<<<< HEAD\n");
    out.push_str(&ours_str);
    if !ours_str.ends_with('\n') && !ours_str.is_empty() {
        out.push('\n');
    }
    out.push_str("=======\n");
    out.push_str(&theirs_str);
    if !theirs_str.ends_with('\n') && !theirs_str.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!(">>>>>>> {}\n", branch_name));

    out.into_bytes()
}

/// Generates an edit script comparing two lists of text lines using the Myers diff algorithm
// for shortest path in a 2d grid.
// Used by the `diff` command to generate line-by-line differences between file versions.
pub fn myers_diff(old_lines: &[&str], new_lines: &[&str]) -> Vec<DiffOp> {
    let n = old_lines.len();
    let m = new_lines.len();
    let max = n + m;

    if max == 0 {
        return Vec::new();
    }

    let mut v = vec![0isize; 2 * max + 1];
    let offset = max as isize;

    let mut trace: Vec<Vec<isize>> = Vec::new();

    for d in 0..=(max as isize) {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize]) {
                v[(k + 1 + offset) as usize]
            } else {
                v[(k - 1 + offset) as usize] + 1
            };
            let mut y = x - k;

            while (x as usize) < n && (y as usize) < m && old_lines[x as usize] == new_lines[y as usize] {
                x += 1;
                y += 1;
            }

            v[(k + offset) as usize] = x;

            if x as usize >= n && y as usize >= m {
                return backtrack_myers(&trace, old_lines, new_lines, n, m);
            }

            k += 2;
        }
    }

    Vec::new()
}

fn backtrack_myers(trace: &[Vec<isize>], old_lines: &[&str], new_lines: &[&str], n: usize, m: usize) -> Vec<DiffOp> {
    let mut x = n as isize;
    let mut y = m as isize;
    let max = n + m;
    let offset = max as isize;

    let mut ops = Vec::new();

    for d in (0..trace.len()).rev() {
        let v = &trace[d];
        let k = x - y;

        let prev_k = if k == -(d as isize) || (k != (d as isize) && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize]) {
            k + 1
        } else {
            k - 1
        };

        let prev_x = v[(prev_k + offset) as usize];
        let prev_y = prev_x - prev_k;

        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            ops.push(DiffOp::Keep(old_lines[x as usize].to_string()));
        }

        if d > 0 {
            if x == prev_x {
                y -= 1;
                ops.push(DiffOp::Insert(new_lines[y as usize].to_string()));
            } else if y == prev_y {
                x -= 1;
                ops.push(DiffOp::Delete(old_lines[x as usize].to_string()));
            }
        }
    }

    ops.reverse();
    ops
}

/// Formats a list of line diff operations into unified diff format (with headers and line hunks).
// Used by `diff` command.
pub fn format_diff_output(path: &str, old_lines: &[&str], new_lines: &[&str], old_label: &str, new_label: &str,) -> String {
    let ops = myers_diff(old_lines, new_lines);

    let mut has_changes = false;
    for op in &ops {
        if matches!(op, DiffOp::Delete(_) | DiffOp::Insert(_)) {
            has_changes = true;
            break;
        }
    }

    if !has_changes {
        return String::new();
    }

    let mut output = String::new();
    output.push_str(&format!("diff --git a/{} b/{}\n", path, path));
    output.push_str(&format!("--- {}\n", old_label));
    output.push_str(&format!("+++ {}\n", new_label));

    let context_size = 3;
    let mut i = 0;

    while i < ops.len() {
        if matches!(ops[i], DiffOp::Keep(_)) {
            i += 1;
            continue;
        }

        let hunk_start = i.saturating_sub(context_size);
        let mut hunk_end = i;

        let mut lookahead = i;
        while lookahead < ops.len() {
            if matches!(ops[lookahead], DiffOp::Delete(_) | DiffOp::Insert(_)) {
                hunk_end = (lookahead + context_size + 1).min(ops.len());
            } else {
                let next_change = (lookahead..ops.len()).find(|&j| matches!(ops[j], DiffOp::Delete(_) | DiffOp::Insert(_)));
                if let Some(nc) = next_change {
                    if nc - lookahead <= 2 * context_size {
                        lookahead = nc;
                        continue;
                    }
                }
                break;
            }
            lookahead += 1;
        }

        let mut old_line_num = 1;
        let mut new_line_num = 1;
        for op in &ops[..hunk_start] {
            match op {
                DiffOp::Keep(_) => {
                    old_line_num += 1;
                    new_line_num += 1;
                }
                DiffOp::Delete(_) => old_line_num += 1,
                DiffOp::Insert(_) => new_line_num += 1,
            }
        }

        let mut old_count = 0;
        let mut new_count = 0;
        for op in &ops[hunk_start..hunk_end] {
            match op {
                DiffOp::Keep(_) => {
                    old_count += 1;
                    new_count += 1;
                }
                DiffOp::Delete(_) => old_count += 1,
                DiffOp::Insert(_) => new_count += 1,
            }
        }

        output.push_str(&format!("@@ -{},{} +{},{} @@\n", old_line_num, old_count, new_line_num, new_count));

        for op in &ops[hunk_start..hunk_end] {
            match op {
                DiffOp::Keep(line) => output.push_str(&format!(" {}\n", line)),
                DiffOp::Delete(line) => output.push_str(&format!("-{}\n", line)),
                DiffOp::Insert(line) => output.push_str(&format!("+{}\n", line)),
            }
        }

        i = hunk_end;
    }

    output
}

/// Runs a git-style three-way tree merge given base/ours/theirs, returning the
/// resulting merged tree plus any paths that conflict and need manual resolution.
/// Identical to the diff loop `merge` uses inline, factored out here so both
/// `cherry_pick` and `cherry_pick_abort` (which has to recompute what an in-progress pick touched) can share it.
// Used by `cherry_pick` and `cherry_pick_abort`.
pub fn three_way_tree_merge(base_tree: &BTreeMap<String, ([u8; 20], u32)>, our_tree: &BTreeMap<String, ([u8; 20], u32)>, their_tree: &BTreeMap<String, ([u8; 20], u32)>) -> Result<(BTreeMap<String, ([u8; 20], u32)>, Vec<MergeConflict>)> {
    let mut all_paths = BTreeSet::new();
    for p in base_tree.keys() { all_paths.insert(p.clone()); }
    for p in our_tree.keys() { all_paths.insert(p.clone()); }
    for p in their_tree.keys() { all_paths.insert(p.clone()); }

    let mut merged_tree: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
    let mut conflicts: Vec<MergeConflict> = Vec::new();

    for path in all_paths {
        let base_entry = base_tree.get(&path);
        let our_entry = our_tree.get(&path);
        let their_entry = their_tree.get(&path);

        if our_entry == their_entry {
            if let Some(entry) = our_entry {
                merged_tree.insert(path, *entry);
            }
        } else if their_entry == base_entry {
            if let Some(entry) = our_entry {
                merged_tree.insert(path, *entry);
            }
        } else if our_entry == base_entry {
            if let Some(entry) = their_entry {
                merged_tree.insert(path, *entry);
            }
        } else {
            let our_bytes = if let Some(our) = our_entry {
                read_object(&hex::encode(our.0))?.1
            } else {
                Vec::new()
            };
            let their_bytes = if let Some(their) = their_entry {
                read_object(&hex::encode(their.0))?.1
            } else {
                Vec::new()
            };
            let base_bytes = if let Some(base) = base_entry {
                read_object(&hex::encode(base.0))?.1
            } else {
                Vec::new()
            };

            conflicts.push(MergeConflict {
                path,
                base: base_bytes,
                ours: our_bytes,
                theirs: their_bytes,
            });
        }
    }

    Ok((merged_tree, conflicts))
}

/// Extracts the full commit message body (everything after the blank line that
/// separates it from the header) from a commit object's raw content.
// Used by `cherry_pick`.
pub fn extract_commit_message(commit_text: &str) -> String {
    let mut in_message = false;
    let mut lines: Vec<&str> = Vec::new();
    for line in commit_text.lines() {
        if in_message {
            lines.push(line);
        } else if line.is_empty() {
            in_message = true;
        }
    }
    lines.join("\n")
}

/// Resolves a commit's parent hashes from its raw object content, in order.
// Used by `cherry_pick`.
pub fn commit_parents(commit_text: &str) -> Vec<String> {
    commit_text
        .lines()
        .filter_map(|line| line.strip_prefix("parent "))
        .map(|s| s.to_string())
        .collect()
}

const STASH_LIST_PATH: &str = ".git/STASH_LIST";

/// Reads all stash entries, most-recent-first (`stash@{0}` == `list[0]`).
pub fn read_stash_list() -> Result<Vec<StashEntry>> {
    if !Path::new(STASH_LIST_PATH).exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(STASH_LIST_PATH).context("Failed to read .git/STASH_LIST")?;
    let mut entries: Vec<StashEntry> = content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let hash = parts.next()?.to_string();
            let message = parts.next().unwrap_or("").to_string();
            Some(StashEntry { hash, message })
        })
        .collect();

    entries.reverse();
    Ok(entries)
}

/// Overwrites the stash list. `entries` must be newest-first (index 0 ==
/// `stash@{0}`), matching what `read_stash_list` returns.
pub fn write_stash_list(entries: &[StashEntry]) -> Result<()> {
    if entries.is_empty() {
        if Path::new(STASH_LIST_PATH).exists() {
            fs::remove_file(STASH_LIST_PATH).context("Failed to remove .git/STASH_LIST")?;
        }
        return Ok(());
    }

    let mut lines: Vec<String> = entries.iter().map(|e| format!("{} {}", e.hash, e.message)).collect();
    lines.reverse();
    let mut content = lines.join("\n");
    content.push('\n');
    fs::write(STASH_LIST_PATH, content).context("Failed to write .git/STASH_LIST")?;
    Ok(())
}

/// Appends a newly-created stash entry as the new `stash@{0}`.
pub fn append_stash_entry(hash: &str, message: &str) -> Result<()> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(STASH_LIST_PATH)
        .context("Failed to open .git/STASH_LIST")?;
    writeln!(file, "{} {}", hash, message)?;
    Ok(())
}

/// Parses a `stash@{N}` reference, a bare `N`, or `None` (meaning `stash@{0}`)
/// into an index into the stash list.
pub fn parse_stash_index(s: Option<&str>) -> Result<usize> {
    match s {
        None => Ok(0),
        Some(raw) => {
            let trimmed = raw.trim();
            let inner = trimmed
                .strip_prefix("stash@{")
                .and_then(|r| r.strip_suffix('}'))
                .unwrap_or(trimmed);
            inner
                .parse::<usize>()
                .map_err(|_| anyhow::anyhow!("fatal: '{}' is not a valid stash reference", raw))
        }
    }
}

/// Human-readable label for the current HEAD, used in stash messages
/// ("WIP on <label>: ...").
pub fn current_branch_label() -> Result<String> {
    match refs::resolve_head()? {
        refs::HeadState::Branch(b) => Ok(b),
        refs::HeadState::Detached(h) => {
            let short = &h[..h.len().min(7)];
            Ok(format!("(detached HEAD at {})", short))
        }
    }
}

/// Builds and writes a tree object directly from a flattened path -> (hash,
/// mode) map, reusing `write_tree_from_index_prefix` via a throwaway
/// `IndexEntry` list. Stat fields are irrelevant here since only the mode and
/// blob hash are encoded into tree entries.
pub fn build_tree_from_map(map: &BTreeMap<String, ([u8; 20], u32)>) -> Result<String> {
    let entries: Vec<IndexEntry> = map
        .iter()
        .map(|(path, (hash, mode))| IndexEntry {
            ctime_secs: 0,
            ctime_nsecs: 0,
            mtime_secs: 0,
            mtime_nsecs: 0,
            dev: 0,
            ino: 0,
            mode: *mode,
            uid: 0,
            gid: 0,
            size: 0,
            hash: *hash,
            path: path.clone(),
        })
        .collect();

    crate::commands::write_tree_from_index_prefix(&entries, "")
}

/// Refuses to proceed unless the working directory and index are currently
/// clean (i.e. exactly match HEAD). `stash apply`/`pop` use this in place of
/// a real three-way merge back into a dirty tree.
pub fn ensure_working_tree_clean(action: &str) -> Result<()> {
    let head_tree: BTreeMap<String, ([u8; 20], u32)> = match refs::resolve_head_commit()? {
        Some(commit_hash) => {
            let tree_hash = tree_hash_of_commit(&commit_hash)?;
            let mut map = BTreeMap::new();
            flatten_tree(&tree_hash, "", &mut map)?;
            map
        }
        None => BTreeMap::new(),
    };

    let index_entries = read_index().unwrap_or_default();
    let index_map: BTreeMap<String, ([u8; 20], u32)> = index_entries
        .iter()
        .map(|e| (e.path.clone(), (e.hash, e.mode)))
        .collect();

    if index_map != head_tree {
        anyhow::bail!(
            "error: cannot {}: you have staged changes.\nPlease commit or reset them first.",
            action
        );
    }

    for (path, (head_hash, _mode)) in &head_tree {
        let path_obj = Path::new(path);
        if !path_obj.exists() {
            anyhow::bail!(
                "error: local file '{}' is missing; cannot safely {}.\nAborting",
                path,
                action
            );
        }

        let content = fs::read(path_obj).with_context(|| format!("Failed to read {}", path))?;
        let hash = hash_content("blob", &content);
        if hash != *head_hash {
            anyhow::bail!(
                "error: Your local changes to the following files would be overwritten by {}:\n\t{}\nPlease commit your changes or stash them before running this command again.\nAborting",
                action,
                path
            );
        }
    }

    Ok(())
}

/// Given a stash index, reads its `w_commit`/`i_commit` pair and flattens
/// both into path -> (hash, mode) maps: `(working_tree, index_tree)`.
pub fn load_stash_trees(idx: usize, list: &[StashEntry]) -> Result<(BTreeMap<String, ([u8; 20], u32)>, BTreeMap<String, ([u8; 20], u32)>)> {
    let w_hash = &list[idx].hash;

    let (obj_type, content) = read_object(w_hash)?;
    if obj_type != "commit" {
        anyhow::bail!("fatal: corrupted stash entry stash@{{{}}}", idx);
    }
    let text = String::from_utf8_lossy(&content).to_string();
    let parents = commit_parents(&text);
    if parents.len() < 2 {
        anyhow::bail!("fatal: corrupted stash entry stash@{{{}}}", idx);
    }
    let head_at_stash = &parents[0];
    let i_hash = &parents[1];

    let w_tree_hash = tree_hash_of_commit(w_hash)?;
    let mut w_tree = BTreeMap::new();
    flatten_tree(&w_tree_hash, "", &mut w_tree)?;

    let i_tree_hash = tree_hash_of_commit(i_hash)?;
    let mut i_tree = BTreeMap::new();
    flatten_tree(&i_tree_hash, "", &mut i_tree)?;

    let _ = head_at_stash; // only needed by `stash show`, kept here for symmetry
    Ok((w_tree, i_tree))
}

/// Shared implementation of `stash apply` and `stash pop`.
pub fn stash_apply_or_pop(stash_ref: Option<String>, drop_after: bool) -> Result<()> {
    let idx = parse_stash_index(stash_ref.as_deref())?;
    let list = read_stash_list()?;
    if list.is_empty() {
        anyhow::bail!("No stash entries found.");
    }
    if idx >= list.len() {
        anyhow::bail!("fatal: stash@{{{}}} is not a valid reference", idx);
    }

    let action = if drop_after { "apply stash (pop)" } else { "apply stash" };
    ensure_working_tree_clean(action)?;

    let (w_tree, i_tree) = load_stash_trees(idx, &list)?;

    let head_tree: BTreeMap<String, ([u8; 20], u32)> = match refs::resolve_head_commit()? {
        Some(commit_hash) => {
            let tree_hash = tree_hash_of_commit(&commit_hash)?;
            let mut map = BTreeMap::new();
            flatten_tree(&tree_hash, "", &mut map)?;
            map
        }
        None => BTreeMap::new(),
    };

    // Remove tracked files that existed at HEAD but were deleted in the
    // stash's working-tree snapshot.
    for path in head_tree.keys() {
        if !w_tree.contains_key(path) {
            let p = Path::new(path);
            if p.exists() {
                let _ = fs::remove_file(p);
            }
        }
    }

    // Write the stash's working-tree snapshot to disk.
    for (path, (hash, mode)) in &w_tree {
        let (_, content) = read_object(&hex::encode(hash))?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, &content).with_context(|| format!("failed to write '{}'", path))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = if (*mode & 0o111) != 0 { 0o755 } else { 0o644 };
            fs::set_permissions(path, fs::Permissions::from_mode(file_mode))?;
        }
    }

    // Rebuild the index to match the stash's index snapshot (preserving the
    // staged vs. unstaged distinction that existed when the stash was made).
    let mut new_entries = crate::commands::build_index_entries_for_tree(&i_tree, true)?;
    write_index(&mut new_entries)?;

    let message = list[idx].message.clone();

    if drop_after {
        let mut remaining: Vec<StashEntry> = list;
        remaining.remove(idx);
        write_stash_list(&remaining)?;
        println!("Dropped stash@{{{}}} ({})", idx, message);
    } else {
        println!("Applied stash@{{{}}} ({})", idx, message);
    }

    Ok(())
}

pub fn read_lines_set(path: &str) -> Result<Vec<String>> {
    if !Path::new(path).exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;
    Ok(content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

pub fn append_line(path: &str, line: &str) -> Result<()> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path))?;
    writeln!(file, "{}", line)?;
    Ok(())
}

/// Collects the full ancestor set of `start` (inclusive), following parent
/// links via BFS.
pub fn collect_ancestors(start: &str) -> Result<HashSet<String>> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(start.to_string());

    while let Some(hash) = queue.pop_front() {
        if visited.contains(&hash) {
            continue;
        }
        visited.insert(hash.clone());

        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            continue;
        }
        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(parent) = line.strip_prefix("parent ") {
                queue.push_back(parent.to_string());
            }
        }
    }

    Ok(visited)
}

/// BFS from `start` over the *entire* ancestor graph (so it can walk through
/// non-candidate commits to reach further candidates), collecting only the
/// commits that are members of `candidates`, in BFS discovery order.
pub fn order_candidates_from(start: &str, candidates: &HashSet<String>) -> Result<Vec<String>> {
    let mut order = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(start.to_string());

    while let Some(hash) = queue.pop_front() {
        if visited.contains(&hash) {
            continue;
        }
        visited.insert(hash.clone());

        if candidates.contains(&hash) {
            order.push(hash.clone());
        }

        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            continue;
        }
        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(parent) = line.strip_prefix("parent ") {
                queue.push_back(parent.to_string());
            }
        }
    }

    Ok(order)
}

/// Detaches HEAD at `hash` and syncs the working directory + index to match
/// it, refusing (like `switch`) if that would clobber local changes. Shared
/// by every point where bisect moves HEAD: stepping to a new midpoint,
/// `bisect reset <commit>`, and restoring a detached-HEAD starting point.
pub fn checkout_bisect_commit(hash: &str) -> Result<()> {
    let target_tree_hash = tree_hash_of_commit(hash)?;
    let mut target_tree = BTreeMap::new();
    flatten_tree(&target_tree_hash, "", &mut target_tree)?;

    let mut head_tree = BTreeMap::new();
    if let Some(current_commit) = refs::resolve_head_commit()? {
        let current_tree_hash = tree_hash_of_commit(&current_commit)?;
        flatten_tree(&current_tree_hash, "", &mut head_tree)?;
    }

    let index_entries = read_index().unwrap_or_default();
    let index_map: BTreeMap<String, [u8; 20]> = index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

    check_switch_safety(&target_tree, &head_tree, &index_map)?;
    sync_working_tree(&target_tree, &head_tree, &index_map)?;
    update_index_from_tree(&target_tree, &head_tree)?;

    refs::set_head_detached(hash)?;
    Ok(())
}

pub fn print_commit_oneline(hash: &str) -> Result<()> {
    let subject = crate::commands::commit_subject_line(hash)?;
    println!("[{}] {}", &hash[..hash.len().min(7)], subject);
    Ok(())
}

pub fn report_first_bad(hash: &str) -> Result<()> {
    let (object_type, content) = read_object(hash)?;
    let text = String::from_utf8_lossy(&content);

    println!("{} is the first bad commit", hash);
    println!("commit {}", hash);
    if object_type == "commit" {
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(author) = line.strip_prefix("author ") {
                println!("Author: {}", author);
            }
        }
    }
    println!();
    println!("    {}", crate::commands::commit_subject_line(hash)?);
    Ok(())
}

pub fn report_outcome(outcome: &BisectOutcome) -> Result<()> {
    match outcome {
        BisectOutcome::WaitingForBad => {
            println!("status: waiting for bad commit, good commit(s) known");
        }
        BisectOutcome::WaitingForGood => {
            println!("status: waiting for good commit(s), bad commit known");
        }
        BisectOutcome::Continue(hash, remaining) => {
            let steps = if *remaining == 0 { 0 } else { (*remaining as f64).log2().ceil() as u32 };
            println!(
                "Bisecting: {} revision{} left to test after this (roughly {} step{})",
                remaining,
                if *remaining == 1 { "" } else { "s" },
                steps,
                if steps == 1 { "" } else { "s" }
            );
            print_commit_oneline(hash)?;
        }
        BisectOutcome::Found(hash) => {
            report_first_bad(hash)?;
        }
    }
    Ok(())
}

/// Recomputes the candidate set from the current BISECT_BAD/GOOD/SKIP state
/// and either checks out the next midpoint (`Continue`) or concludes the
/// search (`Found`). Pure aside from the checkout side effect on `Continue`.
pub fn bisect_recompute() -> Result<BisectOutcome> {
    let bad = match fs::read_to_string(crate::commands::BISECT_BAD_PATH)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(b) => b,
        None => return Ok(BisectOutcome::WaitingForBad),
    };

    let goods = read_lines_set(crate::commands::BISECT_GOOD_PATH)?;
    if goods.is_empty() {
        return Ok(BisectOutcome::WaitingForGood);
    }

    for good in &goods {
        if !is_reachable(&bad, good)? {
            anyhow::bail!(
                "error: some good revs are not ancestors of the bad rev.\n\
                 rgit bisect cannot work properly in this state."
            );
        }
    }

    let ancestors_bad = collect_ancestors(&bad)?;
    let mut ancestors_good: HashSet<String> = HashSet::new();
    for g in &goods {
        ancestors_good.extend(collect_ancestors(g)?);
    }

    let candidates: HashSet<String> = ancestors_bad.difference(&ancestors_good).cloned().collect();

    let skip_set: HashSet<String> = read_lines_set(crate::commands::BISECT_SKIP_PATH)?.into_iter().collect();
    let testable: HashSet<String> = candidates.difference(&skip_set).cloned().collect();

    if testable.is_empty() {
        if candidates.len() <= 1 {
            let hash = candidates.into_iter().next().unwrap_or_else(|| bad.clone());
            return Ok(BisectOutcome::Found(hash));
        }
        anyhow::bail!(
            "error: every commit left to test has been skipped; cannot narrow down further.\n\
             Try marking a different commit good/bad, or reducing the number of skips."
        );
    }

    let order = order_candidates_from(&bad, &testable)?;

    if order.len() == 1 {
        return Ok(BisectOutcome::Found(order[0].clone()));
    }

    let mid = order[order.len() / 2].clone();
    checkout_bisect_commit(&mid)?;

    let remaining = order.len() - 1;
    Ok(BisectOutcome::Continue(mid, remaining))
}

pub fn mark_bad(rev: Option<String>) -> Result<BisectOutcome> {
    if !Path::new(crate::commands::BISECT_START_PATH).exists() {
        anyhow::bail!("fatal: You need to start by \"rgit bisect start\"");
    }

    let target = match rev {
        Some(r) => resolve_commit_from_source(&r)?,
        None => refs::resolve_head_commit()?
            .ok_or_else(|| anyhow::anyhow!("fatal: bad HEAD - I need a HEAD commit"))?,
    };

    fs::write(crate::commands::BISECT_BAD_PATH, format!("{}\n", target)).context("Failed to write .git/BISECT_BAD")?;
    append_line(crate::commands::BISECT_LOG_PATH, &format!("git bisect bad {}", target))?;

    bisect_recompute()
}

pub fn mark_good(rev: Option<String>) -> Result<BisectOutcome> {
    if !Path::new(crate::commands::BISECT_START_PATH).exists() {
        anyhow::bail!("fatal: You need to start by \"rgit bisect start\"");
    }

    let target = match rev {
        Some(r) => resolve_commit_from_source(&r)?,
        None => refs::resolve_head_commit()?
            .ok_or_else(|| anyhow::anyhow!("fatal: bad HEAD - I need a HEAD commit"))?,
    };

    let mut goods = read_lines_set(crate::commands::BISECT_GOOD_PATH)?;
    if !goods.iter().any(|g| g == &target) {
        goods.push(target.clone());
        let mut content = goods.join("\n");
        content.push('\n');
        fs::write(crate::commands::BISECT_GOOD_PATH, content).context("Failed to write .git/BISECT_GOOD")?;
    }
    append_line(crate::commands::BISECT_LOG_PATH, &format!("git bisect good {}", target))?;

    bisect_recompute()
}

pub fn mark_skip(revs: Vec<String>) -> Result<BisectOutcome> {
    if !Path::new(crate::commands::BISECT_START_PATH).exists() {
        anyhow::bail!("fatal: You need to start by \"rgit bisect start\"");
    }

    let targets: Vec<String> = if revs.is_empty() {
        vec![refs::resolve_head_commit()?
            .ok_or_else(|| anyhow::anyhow!("fatal: bad HEAD - I need a HEAD commit"))?]
    } else {
        revs.iter().map(|r| resolve_commit_from_source(r)).collect::<Result<Vec<_>>>()?
    };

    let mut skips = read_lines_set(crate::commands::BISECT_SKIP_PATH)?;
    for target in &targets {
        if !skips.iter().any(|s| s == target) {
            skips.push(target.clone());
        }
        append_line(crate::commands::BISECT_LOG_PATH, &format!("git bisect skip {}", target))?;
    }
    let mut content = skips.join("\n");
    content.push('\n');
    fs::write(crate::commands::BISECT_SKIP_PATH, content).context("Failed to write .git/BISECT_SKIP")?;

    bisect_recompute()
}