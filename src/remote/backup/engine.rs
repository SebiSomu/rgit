//! Helpers of `rgit backup`: where backups go, how they are named, finding this repository's
//! backups in order, and showing paths and sizes.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use crate::remote::backup::types::Backup;

/// The backup directory: `--dir`, else `rgit.backupDir`, else `~/.rgit-backups` (a leading
/// `~/` is the home directory).
// Used by the `backup` command.
pub fn backup_dir(flag: Option<String>) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let chosen = flag.or_else(|| crate::core::config::get_config_value("rgit.backupdir").ok().flatten());
    match chosen {
        Some(dir) => match dir.strip_prefix("~/") {
            Some(rest) => home.map(|h| h.join(rest)),
            None => Some(PathBuf::from(dir)),
        },
        None => home.map(|h| h.join(".rgit-backups")),
    }
}

/// The repository's name for its backup files: the working tree's directory name, with
/// characters other than letters, digits, `.`, `_` and `-` turned into `_`.
// Used by the `backup` command.
pub fn repo_name() -> String {
    let top = crate::workspace::worktree::engine::main_worktree_path();
    let name = top.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".to_string());
    name.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect()
}

/// `2026-11-25-174212` for now (UTC).
// Used by the `backup` command.
pub fn stamp_now() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let secs = now.rem_euclid(86_400);
    format!("{}-{:02}{:02}{:02}", crate::history::story::engine::day(now), secs / 3600, secs / 60 % 60, secs % 60)
}

/// The backups of repository `name` in `dir`, oldest first.
// Used by the `backup` command.
pub fn list_backups(dir: &Path, name: &str) -> Vec<Backup> {
    static NAME: OnceLock<Regex> = OnceLock::new();
    let re = NAME.get_or_init(|| Regex::new(r"^(.*)-(\d{4}-\d{2}-\d{2}-\d{6})(?:-(\d+))?\.bundle$").unwrap());
    let mut out: Vec<Backup> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let file = e.file_name().to_string_lossy().to_string();
                    let caps = re.captures(&file)?;
                    (&caps[1] == name).then(|| Backup { path: e.path(), stamp: caps[2].to_string(), sequence: caps.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(1) })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.stamp.cmp(&b.stamp).then(a.sequence.cmp(&b.sequence)));
    out
}

/// A path for showing, with the home directory as `~` and `/` separators.
// Used by the `backup` command.
pub fn shown(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = home.to_string_lossy().replace('\\', "/");
        if let Some(rest) = text.strip_prefix(&home) {
            return format!("~{}", rest);
        }
    }
    text
}

/// `12 KB` / `3.4 MB` / `512 bytes`.
// Used by the `backup` command.
pub fn size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{} KB", b.div_ceil(1 << 10)),
        b => format!("{} bytes", b),
    }
}
