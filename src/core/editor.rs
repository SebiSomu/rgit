use anyhow::Context;
use std::path::Path;
use std::process::Command;
use crate::core::config::get_config_value;

/// Launches the configured editor synchronously on the specified file.
// Used by `rebase`.
pub fn run_editor(file: &Path, sequence: bool) -> anyhow::Result<()> {
    let command = editor_command(sequence)?;
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("fatal: empty editor command"))?;
    let status = Command::new(program)
        .args(parts)
        .arg(file)
        .status()
        .with_context(|| format!("failed to launch editor '{}'", command))?;
    if !status.success() {
        anyhow::bail!("error: editor '{}' exited with {}", command, status);
    }
    Ok(())
}

/// Resolves the command string to invoke the user's configured editor.
// Used by `rebase`.
pub fn editor_command(sequence: bool) -> anyhow::Result<String> {
    let nonempty = |v: Option<String>| v.filter(|s| !s.trim().is_empty());

    if sequence {
        if let Some(e) = nonempty(std::env::var("RGIT_SEQUENCE_EDITOR").ok()) {
            return Ok(e);
        }
    }
    if let Some(e) = nonempty(std::env::var("RGIT_EDITOR").ok()) {
        return Ok(e);
    }
    if let Some(e) = nonempty(get_config_value("core.editor")?) {
        return Ok(e);
    }
    for var in ["VISUAL", "EDITOR"] {
        if let Some(e) = nonempty(std::env::var(var).ok()) {
            return Ok(e);
        }
    }
    Ok(if cfg!(windows) { "notepad" } else { "vi" }.to_string())
}
