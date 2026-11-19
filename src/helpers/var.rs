use anyhow::Result;

use crate::core::commit::current_signature;
use crate::core::config::{get_config_value, global_config_path};
use crate::helpers::rebase::editor_command;

/// Usage text of `var`, printed for `-h` and usage errors.
pub const VAR_USAGE: &str = "usage: rgit var (-l | <variable>)\n";

/// The logical variables `var` knows, in the order `var -l` lists them.
pub const VAR_NAMES: [&str; 8] = [
    "GIT_COMMITTER_IDENT",
    "GIT_AUTHOR_IDENT",
    "GIT_EDITOR",
    "GIT_SEQUENCE_EDITOR",
    "GIT_PAGER",
    "GIT_DEFAULT_BRANCH",
    "GIT_SHELL_PATH",
    "GIT_CONFIG_GLOBAL",
];

/// The identity rgit would record right now, as it appears in a commit:
/// `Name <email> <unix-time> <+HHMM>`.
// Used by `var_value` for `GIT_AUTHOR_IDENT` and `GIT_COMMITTER_IDENT`.
fn ident_line() -> Result<String> {
    let sig = current_signature()?;
    Ok(format!("{} <{}> {} {}", sig.name, sig.email, sig.timestamp, sig.tz))
}

/// The pager rgit's settings name: `RGIT_PAGER`, `core.pager`, `PAGER`, else `less`.
// Used by `var_value` for `GIT_PAGER`.
fn pager_command() -> String {
    let nonempty = |v: Option<String>| v.filter(|s| !s.trim().is_empty());
    nonempty(std::env::var("RGIT_PAGER").ok())
        .or_else(|| nonempty(get_config_value("core.pager").ok().flatten()))
        .or_else(|| nonempty(std::env::var("PAGER").ok()))
        .unwrap_or_else(|| "less".to_string())
}

/// Value of one logical variable: `Ok(None)` for a name `var` does not know or a variable
/// without a value (no global config location), an error when the value cannot be worked out.
// Used by `var`.
pub fn var_value(name: &str) -> Result<Option<String>> {
    Ok(match name {
        "GIT_COMMITTER_IDENT" | "GIT_AUTHOR_IDENT" => Some(ident_line()?),
        "GIT_EDITOR" => Some(editor_command(false)?),
        "GIT_SEQUENCE_EDITOR" => Some(editor_command(true)?),
        "GIT_PAGER" => Some(pager_command()),
        "GIT_DEFAULT_BRANCH" => Some(get_config_value("init.defaultbranch")?.filter(|v| !v.trim().is_empty()).unwrap_or_else(|| "main".to_string())),
        "GIT_SHELL_PATH" => Some(if cfg!(windows) { "sh".to_string() } else { "/bin/sh".to_string() }),
        "GIT_CONFIG_GLOBAL" => global_config_path().map(|p| p.to_string_lossy().to_string()),
        _ => None,
    })
}
