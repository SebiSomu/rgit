use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::core::config::author_identity;
use crate::core::object_store::{flatten_tree, hash_content, read_object, tree_hash_of_commit, write_object};
use crate::core::index::{build_entry, read_index, write_index};
use crate::objects::CommitSignature;
use crate::core::refs;

/// Resolves a commit's parent hashes from its raw object content, in order.
// Used by `cherry_pick`.
pub fn commit_parents(commit_text: &str) -> Vec<String> {
    commit_text
        .lines()
        .filter_map(|line| line.strip_prefix("parent "))
        .map(|s| s.to_string())
        .collect()
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

/// Resolves any revision expression `rev-parse` understands (branch, tag, `heads/<name>`,
/// hash or prefix, `~N` / `^N` chains, ...) into the hash of the commit it names, peeling
/// annotated tags.
// Used by `merge`, `diff`, `restore`, and tree resolution helpers.
pub fn resolve_commit_from_source(source: &str) -> anyhow::Result<String> {
    let hash = match crate::core::rev_parse::resolve_rev(source) {
        Ok(hash) => hash,
        // Keep precise failures ("commit ... has no parent"); an unknown name is an invalid reference.
        Err(e) if e.to_string().starts_with("error:") => return Err(e),
        Err(_) => anyhow::bail!("error: invalid reference: '{}'", source),
    };
    match crate::core::rev_parse::peel_to_type(hash.clone(), "commit") {
        Ok(commit) => Ok(commit),
        Err(_) => {
            let (obj_type, _) = read_object(&hash)?;
            anyhow::bail!("error: '{}' is not a commit (it is a {})", source, obj_type);
        }
    }
}

/// Resolves a source branch/commit name/hash into its tree structure, returning
/// a map of paths to their object hashes and modes.
// Used by `restore`, `diff`, and `merge` commands.
pub fn resolve_tree_from_source(source: &str) -> anyhow::Result<BTreeMap<String, ([u8; 20], u32)>> {
    let commit_hash = resolve_commit_from_source(source)?;
    let tree_hash = tree_hash_of_commit(&commit_hash)?;
    let mut tree_map = BTreeMap::new();
    flatten_tree(&tree_hash, "", &mut tree_map)?;
    Ok(tree_map)
}

/// Traverses parent commit hashes via BFS to check if the target commit is reachable
/// from the start commit.
// Used by `branch -d` safety checks to verify merge status.
pub fn is_reachable(start: &str, target: &str) -> anyhow::Result<bool> {
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

/// Returns the first line of a commit's message body (its "subject line").
// Used by `reset --hard` to print the familiar `HEAD is now at <hash> <subject>` line.
pub fn commit_subject_line(commit_hash: &str) -> anyhow::Result<String> {
    let (object_type, content) = read_object(commit_hash)?;
    if object_type != "commit" {
        anyhow::bail!("{} is not a commit object", commit_hash);
    }

    let text = String::from_utf8_lossy(&content);
    let mut in_message = false;
    for line in text.lines() {
        if in_message {
            return Ok(line.to_string());
        } else if line.is_empty() {
            in_message = true;
        }
    }

    Ok(String::new())
}

/// Returns the abbreviated 7-character prefix of an object hash.
// Used by `commit`, `rebase`, and `banana_split`.
pub fn short_hash(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

/// Constructs and writes a commit object from a tree hash, optional parent hash, and commit message.
// Used by `commit`, `rebase`, `banana_split`, `cherry_pick`, and `revert`.
pub fn build_commit(tree_hash: String, parent_hash: Option<String>, message: &str) -> anyhow::Result<String> {
    let mut content = format!("tree {}\n", tree_hash);
    if let Some(parent) = parent_hash {
        content.push_str(&format!("parent {}\n", parent));
    }

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let author = author_identity()?;

    content.push_str(&format!("author {} {} +0000\n", author, timestamp));
    content.push_str(&format!("committer {} {} +0000\n", author, timestamp));
    content.push_str(&format!("\n{}\n", message));

    write_object("commit", content.as_bytes())
}

/// The committer: the configured `Name <email>` identity stamped with the current time (in
/// UTC), each part overridable like git with `GIT_COMMITTER_NAME`, `GIT_COMMITTER_EMAIL` and
/// `GIT_COMMITTER_DATE`.
// Used by `commit`, `merge`, `cherry-pick` and everything else that records commits.
pub fn current_signature() -> anyhow::Result<CommitSignature> {
    signature_from_env("COMMITTER")
}

/// The default author: like `current_signature`, from `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`
/// and `GIT_AUTHOR_DATE` when set.
// Used by `commit`.
pub fn current_author_signature() -> anyhow::Result<CommitSignature> {
    signature_from_env("AUTHOR")
}

fn signature_from_env(role: &str) -> anyhow::Result<CommitSignature> {
    let identity = author_identity()?;
    let (mut name, mut email) = identity.split_once(" <").map(|(n, e)| (n.to_string(), e.trim_end_matches('>').to_string())).unwrap_or((identity, String::new()));
    let var = |part: &str| std::env::var(format!("GIT_{}_{}", role, part)).ok();
    if let Some(value) = var("NAME") {
        name = value;
    }
    if let Some(value) = var("EMAIL") {
        email = value;
    }
    let (timestamp, tz) = match var("DATE").filter(|d| !d.trim().is_empty()) {
        Some(date) => parse_commit_date(date.trim_start_matches('@')).map_err(|_| anyhow::anyhow!("fatal: invalid date format: {}", date))?,
        None => (SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64, "+0000".to_string()),
    };
    Ok(CommitSignature { name, email, timestamp, tz })
}

/// Prints git's commit summary: `[<branch> <abbrev>] <subject>`, then ` Author:` when the
/// author is not the committer, ` Date:` when `show_date` (an author taken from another commit,
/// or `--date`), and for non-merges the diffstat line and create/delete-mode summary.
// Used by `commit` and `cherry-pick`.
pub fn print_commit_summary(head_state: &refs::HeadState, commit_hash: &str, author: &CommitSignature, committer: &CommitSignature, show_date: bool) -> anyhow::Result<()> {
    use crate::helpers::format_patch::{parse_commit_object, split_commit_message};
    let commit = parse_commit_object(commit_hash)?;
    let label = match head_state {
        refs::HeadState::Branch(branch) => branch.clone(),
        refs::HeadState::Detached(_) => "detached HEAD".to_string(),
    };
    let root = if commit.parents.is_empty() { " (root-commit)" } else { "" };
    let subject = split_commit_message(&commit.message).0;
    println!("[{}{} {}] {}", label, root, &commit_hash[..7], subject);
    if author.name != committer.name || author.email != committer.email {
        println!(" Author: {} <{}>", author.name, author.email);
    }
    if show_date {
        println!(" Date: {}", format_commit_date(author.timestamp, &author.tz));
    }
    if commit.parents.len() <= 1 {
        // git's summary: `--shortstat --summary` against the parent, renames detected.
        use crate::helpers::diff_plumbing::{diff_trees, render_diff_changes};
        let opts = crate::objects::DiffPlumbingOptions {
            recursive: true,
            shortstat: true,
            summary: true,
            renames: crate::helpers::rename::default_rename_options("diff.renames"),
            ..Default::default()
        };
        let parent_tree = match commit.parents.first() {
            Some(parent) => Some(parse_commit_object(parent)?.tree),
            None => None,
        };
        let mut changes = Vec::new();
        diff_trees(parent_tree.as_deref(), Some(&commit.tree), "", &opts, &[], &mut changes)?;
        let changes = crate::helpers::rename::detect_renames(changes, &opts.renames, &[]);
        print!("{}", render_diff_changes(&changes, &opts)?);
    }
    Ok(())
}

/// Parses `commit --author="Name <email>"` into its name and email.
// Used by `commit --author`.
pub fn parse_author_option(value: &str) -> anyhow::Result<(String, String)> {
    let parsed = value.trim().strip_suffix('>').and_then(|rest| rest.rsplit_once('<')).map(|(name, email)| (name.trim().to_string(), email.trim().to_string()));
    match parsed {
        Some((name, email)) if !name.is_empty() && !name.contains(['<', '>', '\n']) && !email.contains(['<', '>', '\n']) => Ok((name, email)),
        _ => anyhow::bail!("fatal: --author '{}' is not 'Name <email>' and matches no existing author", value),
    }
}

/// Parses `commit --date=<date>` into a timestamp and timezone: git's raw `<unix> <+HHMM>`
/// form, or anything `rev-list --since` accepts (ISO dates, `2.days.ago`, `@<unix>`), whose
/// explicit `+HHMM` zone is kept (UTC otherwise).
// Used by `commit --date`.
pub fn parse_commit_date(value: &str) -> anyhow::Result<(i64, String)> {
    let value = value.trim();
    if let Some((ts, tz)) = value.split_once(' ') {
        if let Ok(ts) = ts.parse::<i64>() {
            if tz.len() == 5 && (tz.starts_with('+') || tz.starts_with('-')) && tz[1..].chars().all(|c| c.is_ascii_digit()) {
                return Ok((ts, tz.to_string()));
            }
        }
    }
    let ts = crate::helpers::rev_list::parse_approx_date(value).ok_or_else(|| anyhow::anyhow!("fatal: invalid date format: {}", value))?;
    let tz = value
        .rsplit(|c: char| c == ' ' || c == 'T')
        .next()
        .filter(|z| z.len() == 5 && (z.starts_with('+') || z.starts_with('-')) && z[1..].chars().all(|c| c.is_ascii_digit()))
        .unwrap_or("+0000");
    Ok((ts, tz.to_string()))
}

/// Cleans up a commit message like git's default cleanup: trailing whitespace is removed from
/// every line, runs of blank lines are collapsed, and leading/trailing blank lines are dropped;
/// with `strip_comments` (messages that went through the editor) lines starting with `#` go
/// too. The result ends with a single newline, or is empty.
// Used by `commit`.
pub fn cleanup_commit_message(text: &str, strip_comments: bool) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        if strip_comments && line.starts_with('#') {
            continue;
        }
        let line = line.trim_end();
        if line.is_empty() && out.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    if out.is_empty() { String::new() } else { format!("{}\n", out.join("\n")) }
}

/// Appends a `Signed-off-by: Name <email>` trailer, separated by a blank line unless the
/// message already ends in a trailer block, and not repeated if it is already the last line.
// Used by `commit -s`.
pub fn add_signoff(message: &str, signer: &CommitSignature) -> String {
    let trailer = format!("Signed-off-by: {} <{}>", signer.name, signer.email);
    let body = message.trim_end_matches('\n');
    if body.lines().last() == Some(trailer.as_str()) {
        return format!("{}\n", body);
    }
    let last_paragraph = body.rsplit("\n\n").next().unwrap_or("");
    let is_trailer = |l: &str| l.split_once(": ").is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    let ends_in_trailers = body.contains("\n\n") && last_paragraph.lines().all(is_trailer);
    if body.is_empty() {
        format!("\n{}\n", trailer)
    } else if ends_in_trailers {
        format!("{}\n{}\n", body, trailer)
    } else {
        format!("{}\n\n{}\n", body, trailer)
    }
}

/// Writes a commit object with any number of parents and explicit author and committer.
// Used by `commit`.
pub fn write_commit_object(tree: &str, parents: &[String], author: &CommitSignature, committer: &CommitSignature, message: &str) -> anyhow::Result<String> {
    let mut content = format!("tree {}\n", tree);
    for parent in parents {
        content.push_str(&format!("parent {}\n", parent));
    }
    content.push_str(&format!("author {} <{}> {} {}\n", author.name, author.email, author.timestamp, author.tz));
    content.push_str(&format!("committer {} <{}> {} {}\n", committer.name, committer.email, committer.timestamp, committer.tz));
    content.push('\n');
    content.push_str(message);
    write_object("commit", content.as_bytes())
}

/// Lists tracked files whose working-tree copy differs from the index: `('M', path)` for a
/// modified file and `('D', path)` for a deleted one, in path order.
// Used by `commit -a` and `commit`'s "no changes added to commit" report.
pub fn tracked_worktree_changes() -> anyhow::Result<Vec<(char, String)>> {
    let mut changes: Vec<(char, String)> = Vec::new();
    for entry in read_index().unwrap_or_default() {
        // An unmerged path is always a change: its working-tree file (or its absence)
        // resolves the conflict.
        if entry.stage != 0 {
            if changes.last().map(|c| &c.1) != Some(&entry.path) {
                let kind = if std::path::Path::new(&entry.path).is_file() { 'M' } else { 'D' };
                changes.push((kind, entry.path));
            }
            continue;
        }
        // Submodules are staged with `add`, not picked up by `commit -a`.
        if entry.mode == 0o160000 {
            continue;
        }
        match fs::read(&entry.path) {
            Ok(content) => {
                if hash_content("blob", &content) != entry.hash {
                    changes.push(('M', entry.path));
                }
            }
            // Left out by a sparse checkout: absent on purpose, not deleted.
            Err(_) if entry.skip_worktree => {}
            Err(_) => changes.push(('D', entry.path)),
        }
    }
    changes.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(changes)
}

/// Stages every tracked file's working-tree state (modified files re-hashed, deleted files
/// removed from the index); untracked files are left alone.
// Used by `commit -a`.
pub fn stage_tracked_changes() -> anyhow::Result<()> {
    let changes = tracked_worktree_changes()?;
    if changes.is_empty() {
        return Ok(());
    }
    let mut entries = read_index().unwrap_or_default();
    for (kind, path) in changes {
        entries.retain(|e| e.path != path);
        if kind == 'M' {
            let content = fs::read(&path)?;
            let hash = hash_content("blob", &content);
            write_object("blob", &content)?;
            entries.push(build_entry(&path, hash, &fs::metadata(&path)?));
        }
    }
    write_index(&mut entries)
}

/// Formats a commit date the way `git commit` prints it (`Sat Oct 24 12:19:34 2026 +0300`).
// Used by `commit`'s ` Date:` line.
pub fn format_commit_date(timestamp: i64, tz: &str) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let local = timestamp + crate::helpers::format_patch::tz_offset_seconds(tz);
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let (year, month, day) = crate::helpers::format_patch::civil_from_days(days);
    format!(
        "{} {} {} {:02}:{:02}:{:02} {} {}",
        DAYS[days.rem_euclid(7) as usize],
        MONTHS[(month - 1) as usize],
        day,
        secs / 3600,
        secs % 3600 / 60,
        secs % 60,
        year,
        tz
    )
}

/// Opens the configured editor on `.git/COMMIT_EDITMSG`, prefilled with `initial` and a
/// commented help block, and returns what was saved (uncleaned).
// Used by `commit` when no message is given, or with `-e` / `--amend` / a merge in progress.
pub fn edit_commit_message(initial: &str, comments: &str) -> anyhow::Result<String> {
    let path = crate::core::repo::path(".git/COMMIT_EDITMSG");
    let initial = initial.trim_end_matches('\n');
    let mut text = if initial.is_empty() { String::new() } else { format!("{}\n", initial) };
    text.push_str("\n# Please enter the commit message for your changes. Lines starting\n# with '#' will be ignored, and an empty message aborts the commit.\n#\n");
    text.push_str(comments);
    fs::write(&path, text)?;
    crate::helpers::rebase::run_editor(&path, false)?;
    Ok(fs::read_to_string(&path)?)
}
