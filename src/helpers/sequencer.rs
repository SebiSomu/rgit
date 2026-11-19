//! git's sequencer state for multi-commit `cherry-pick`: `.git/sequencer/{todo,head,
//! abort-safety,opts}`, plus the commit-message trailers `-x` and `-s` add.

use std::fs;
use crate::objects::CherryPickOptions;

fn seq_path(name: &str) -> std::path::PathBuf {
    crate::core::repo::path(format!(".git/sequencer/{}", name))
}

/// Whether a multi-commit sequence is in progress (`.git/sequencer` exists).
// Used by `cherry-pick`.
pub fn sequencer_active() -> bool {
    crate::core::repo::path(".git/sequencer").is_dir()
}

/// Creates `.git/sequencer` and records the starting `HEAD` (`head` and `abort-safety`) and
/// the options (`opts`, git's config format and key order).
// Used by `cherry-pick` when picking more than one commit.
pub fn start_sequence(head: &str, opts: &CherryPickOptions) -> anyhow::Result<()> {
    fs::create_dir_all(crate::core::repo::path(".git/sequencer"))?;
    fs::write(seq_path("head"), format!("{}\n", head))?;
    update_abort_safety(head)?;
    // Like git, `opts` exists only when some option is set.
    let mut text = String::from("[options]\n");
    let mut flag = |set: bool, key: &str| {
        if set {
            text.push_str(&format!("\t{} = true\n", key));
        }
    };
    flag(opts.no_commit, "no-commit");
    flag(opts.allow_empty, "allow-empty");
    flag(opts.allow_empty_message, "allow-empty-message");
    flag(opts.keep_redundant, "keep-redundant-commits");
    flag(opts.signoff, "signoff");
    flag(opts.record_origin, "record-origin");
    flag(opts.ff, "allow-ff");
    if let Some(mainline) = opts.mainline {
        text.push_str(&format!("\tmainline = {}\n", mainline));
    }
    if text != "[options]\n" {
        fs::write(seq_path("opts"), text)?;
    }
    Ok(())
}

/// Reads back the options saved by `start_sequence`.
// Used by `cherry-pick --continue` / `--skip`.
pub fn read_sequence_options() -> CherryPickOptions {
    let mut opts = CherryPickOptions::default();
    let text = fs::read_to_string(seq_path("opts")).unwrap_or_default();
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once(" = ").or_else(|| line.trim().split_once('=')) else { continue };
        let (key, value) = (key.trim(), value.trim());
        let on = value == "true";
        match key {
            "no-commit" => opts.no_commit = on,
            "allow-empty" => opts.allow_empty = on,
            "allow-empty-message" => opts.allow_empty_message = on,
            "keep-redundant-commits" => opts.keep_redundant = on,
            "signoff" => opts.signoff = on,
            "record-origin" => opts.record_origin = on,
            "allow-ff" => opts.ff = on,
            "mainline" => opts.mainline = value.parse().ok(),
            _ => {}
        }
    }
    opts
}

/// Writes the remaining commits as `pick <abbrev> <subject>` lines.
// Used by `cherry-pick`.
pub fn save_todo(commits: &[String]) -> anyhow::Result<()> {
    let mut text = String::new();
    for commit in commits {
        let parsed = crate::helpers::format_patch::parse_commit_object(commit)?;
        let subject = crate::helpers::format_patch::split_commit_message(&parsed.message).0;
        text.push_str(&format!("pick {} {}\n", crate::helpers::rev_list::abbreviate_hash(commit), subject));
    }
    fs::write(seq_path("todo"), text)?;
    Ok(())
}

/// The commits left in the todo list, in order (blank lines and `#` comments ignored).
// Used by `cherry-pick --continue` / `--skip`.
pub fn read_todo() -> anyhow::Result<Vec<String>> {
    let text = fs::read_to_string(seq_path("todo")).map_err(|_| anyhow::anyhow!("error: could not read '.git/sequencer/todo'"))?;
    let mut commits = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let mut parts = line.split_whitespace();
        let (command, rev) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        if command != "pick" && command != "p" {
            anyhow::bail!("error: invalid line: {}", line);
        }
        commits.push(crate::core::commit::resolve_commit_from_source(rev).map_err(|_| anyhow::anyhow!("error: could not parse '{}'", rev))?);
    }
    Ok(commits)
}

/// The `HEAD` the sequence started from.
// Used by `cherry-pick --abort`.
pub fn sequence_head() -> Option<String> {
    fs::read_to_string(seq_path("head")).ok().map(|h| h.trim().to_string()).filter(|h| !h.is_empty())
}

/// Records the commit `HEAD` should be at, so `--abort` can tell if it was moved meanwhile.
// Used by `cherry-pick`.
pub fn update_abort_safety(head: &str) -> anyhow::Result<()> {
    if sequencer_active() {
        fs::write(seq_path("abort-safety"), format!("{}\n", head))?;
    }
    Ok(())
}

/// Whether `HEAD` is still where the sequence last left it.
// Used by `cherry-pick --abort`.
pub fn rollback_is_safe(head: &str) -> bool {
    match fs::read_to_string(seq_path("abort-safety")) {
        Ok(saved) => saved.trim() == head,
        Err(_) => true,
    }
}

/// Removes `.git/sequencer`.
// Used by `cherry-pick`, `commit` and `reset`.
pub fn remove_sequence() {
    let _ = fs::remove_dir_all(crate::core::repo::path(".git/sequencer"));
}

/// git's `sequencer_post_commit_cleanup`: once a commit (or reset) concludes a conflicted
/// pick, `CHERRY_PICK_HEAD` goes, and so does the sequence when that pick was its last one.
// Used by `commit` and `reset`.
pub fn post_commit_cleanup() {
    let pick_head = crate::core::repo::path(".git/CHERRY_PICK_HEAD");
    if !pick_head.exists() {
        return;
    }
    let _ = fs::remove_file(pick_head);
    let remaining = fs::read_to_string(seq_path("todo")).map(|t| t.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).count()).unwrap_or(0);
    if remaining <= 1 {
        remove_sequence();
    }
}

/// git's `has_conforming_footer`: the message's last paragraph (never the subject) is made
/// only of `Key: value` trailers or `(cherry picked from commit ...)` lines.
fn ends_in_trailers(message: &str) -> bool {
    let body = message.trim_end_matches('\n');
    let Some((_, last)) = body.rsplit_once("\n\n") else { return false };
    let is_trailer = |l: &str| {
        l.starts_with("(cherry picked from commit ")
            || l.split_once(':').is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    };
    !last.trim().is_empty() && last.lines().all(is_trailer)
}

/// Appends `(cherry picked from commit <hash>)` (`-x`), after a blank line unless the message
/// already ends in trailers.
// Used by `cherry-pick -x`.
pub fn append_cherry_picked_from(message: &str, commit: &str) -> String {
    let mut text = message.trim_end_matches('\n').to_string();
    text.push('\n');
    if !ends_in_trailers(&text) {
        text.push('\n');
    }
    text.push_str(&format!("(cherry picked from commit {})\n", commit));
    text
}

/// Appends `Signed-off-by: <committer>` (`-s`) the same way, unless it is already the last line.
// Used by `cherry-pick -s`.
pub fn append_signoff(message: &str, signer: &crate::objects::CommitSignature) -> String {
    let trailer = format!("Signed-off-by: {} <{}>", signer.name, signer.email);
    let mut text = message.trim_end_matches('\n').to_string();
    if text.lines().last() == Some(trailer.as_str()) {
        return format!("{}\n", text);
    }
    text.push('\n');
    if !ends_in_trailers(&text) {
        text.push('\n');
    }
    text.push_str(&trailer);
    text.push('\n');
    text
}
