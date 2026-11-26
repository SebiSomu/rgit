//! Engines of `rgit credential` and `rgit credential-store`: git's credential protocol
//! (helpers from `credential.helper`, terminal and askpass prompts), the `~/.git-credentials`
//! store, and the hand-off that lets GitHub commands approve or reject the token they used.

use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use crate::remote::credential::types::Credential;

/// The helpers listed in `credential.helper`, global config first. An empty value clears
/// the helpers listed before it, as in git.
// Used by `fill`, `approve`, `reject` and `approve_pending`.
pub fn configured_helpers() -> Vec<String> {
    let mut helpers = Vec::new();
    for (key, value) in crate::core::config::read_all_config_entries() {
        if key.eq_ignore_ascii_case("credential.helper") {
            if value.trim().is_empty() {
                helpers.clear();
            } else {
                helpers.push(value.trim().to_string());
            }
        }
    }
    helpers
}

/// Asks the helpers, in order, for the missing username and password, then prompts for
/// whatever is still missing when `prompt` is set. Returns whether anything was typed.
/// `Err` holds a `fatal:` message (without the prefix): a missing host or protocol, a helper
/// that said `quit`, or a prompt that could not be shown.
// Used by `credential fill` and `resolve_github_token`.
pub fn fill(credential: &mut Credential, prompt: bool) -> Result<bool, String> {
    check_required_fields(credential)?;
    for helper in configured_helpers() {
        if credential.is_complete() {
            break;
        }
        if let Some(answer) = run_helper(&helper, "get", credential) {
            credential.merge_from(answer);
        }
        if credential.quit {
            return Err(format!("credential helper '{}' told us to quit", helper));
        }
    }
    if credential.is_complete() || !prompt {
        return Ok(false);
    }
    if credential.username.is_none() {
        credential.username = Some(ask("Username", &credential.describe(false), false)?);
    }
    if credential.password.is_none() {
        credential.password = Some(ask("Password", &credential.describe(true), true)?);
    }
    Ok(true)
}

/// Tells every helper to remember a login that worked (`store`). A credential without both
/// a username and a password is not stored.
// Used by `credential approve` and `approve_pending`.
pub fn approve(credential: &Credential) -> Result<(), String> {
    check_required_fields(credential)?;
    if !credential.is_complete() {
        return Ok(());
    }
    for helper in configured_helpers() {
        run_helper(&helper, "store", credential);
    }
    Ok(())
}

/// Tells every helper to forget a login that was refused (`erase`).
// Used by `credential reject` and `reject_pending`.
pub fn reject(credential: &Credential) -> Result<(), String> {
    check_required_fields(credential)?;
    for helper in configured_helpers() {
        run_helper(&helper, "erase", credential);
    }
    Ok(())
}

/// Git refuses to send a credential without a host or protocol to anyone (CVE-2020-11008).
// Used by `fill`, `approve` and `reject`.
fn check_required_fields(credential: &Credential) -> Result<(), String> {
    if credential.host.is_none() {
        return Err("refusing to work with credential missing host field".to_string());
    }
    if credential.protocol.is_none() {
        return Err("refusing to work with credential missing protocol field".to_string());
    }
    Ok(())
}

/// Runs one helper with `action` (`get`, `store` or `erase`), the credential on its stdin.
/// `store` (with `--file <path>`) runs in-process; `!<shell>` runs through the shell; an
/// absolute path runs that program; any other `<name>` runs `git-credential-<name>`, or
/// `git credential-<name>` so helpers shipped with git (manager, cache, libsecret) work.
/// Returns the helper's answer for a successful `get`.
// Used by `fill`, `approve` and `reject`.
pub fn run_helper(helper: &str, action: &str, credential: &Credential) -> Option<Credential> {
    let words: Vec<&str> = helper.split_whitespace().collect();
    if words.first() == Some(&"store") {
        let files = match parse_store_options(&words[1..].iter().map(|w| w.to_string()).collect::<Vec<_>>()) {
            Ok((files, _)) => files,
            Err(_) => {
                eprintln!("warning: invalid options for credential helper 'store': {}", helper);
                return None;
            }
        };
        return store_action(&files, action, credential).ok().flatten();
    }

    let mut commands: Vec<Command> = Vec::new();
    if let Some(script) = helper.strip_prefix('!') {
        let line = format!("{} {}", script, action);
        let mut sh = Command::new("sh");
        sh.arg("-c").arg(&line);
        commands.push(sh);
        if cfg!(windows) {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(&line);
            commands.push(cmd);
        }
    } else if std::path::Path::new(words[0]).is_absolute() {
        let mut program = Command::new(words[0]);
        program.args(&words[1..]).arg(action);
        commands.push(program);
    } else {
        let mut direct = Command::new(format!("git-credential-{}", words[0]));
        direct.args(&words[1..]).arg(action);
        commands.push(direct);
        let mut via_git = Command::new("git");
        via_git.arg(format!("credential-{}", words[0])).args(&words[1..]).arg(action);
        commands.push(via_git);
    }

    let input = credential.to_protocol_text();
    for mut command in commands {
        let child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                eprintln!("warning: cannot run credential helper '{}': {}", helper, e);
                return None;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
        let output = child.wait_with_output().ok()?;
        if action != "get" || !output.status.success() {
            return None;
        }
        return Credential::parse(&String::from_utf8_lossy(&output.stdout)).ok();
    }
    eprintln!("warning: credential helper '{}' not found", helper);
    None
}

/// Asks for one field: through the program in `GIT_ASKPASS` or `core.askPass` when set,
/// else on the terminal (without echo for the password). Fails like git when
/// `GIT_TERMINAL_PROMPT=0` or there is no terminal.
// Used by `fill`.
fn ask(what: &str, url: &str, hidden: bool) -> Result<String, String> {
    let prompt = format!("{} for '{}': ", what, url);
    if let Some(program) = askpass_program() {
        let output = Command::new(&program).arg(&prompt).stdin(Stdio::null()).stderr(Stdio::inherit()).output();
        return match output {
            Ok(output) if output.status.success() => {
                Ok(String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or_default().to_string())
            }
            _ => Err(format!("could not read {} for '{}': askpass failed", what, url)),
        };
    }
    if terminal_prompts_disabled() {
        return Err(format!("could not read {} for '{}': terminal prompts disabled", what, url));
    }
    let no_terminal = |e: std::io::Error| {
        let reason = e.to_string();
        let reason = reason.split(" (os error").next().unwrap_or(&reason).to_string();
        format!("could not read {} for '{}': {}", what, url, reason)
    };
    if hidden {
        return rpassword::prompt_password(&prompt).map_err(no_terminal);
    }
    let (input, mut output) = open_terminal().map_err(no_terminal)?;
    output.write_all(prompt.as_bytes()).and_then(|_| output.flush()).map_err(no_terminal)?;
    let mut line = String::new();
    std::io::BufReader::new(input).read_line(&mut line).map_err(no_terminal)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// The program that answers prompts instead of the terminal: `GIT_ASKPASS`, else
/// `core.askPass` (empty values don't count).
// Used by `ask` and `resolve_github_token`.
pub fn askpass_program() -> Option<String> {
    std::env::var("GIT_ASKPASS")
        .ok()
        .filter(|program| !program.trim().is_empty())
        .or_else(|| crate::core::config::get_config_value("core.askpass").ok().flatten())
        .map(|program| program.trim().to_string())
        .filter(|program| !program.is_empty())
}

/// Whether `GIT_TERMINAL_PROMPT` turns terminal prompts off.
// Used by `ask` and `resolve_github_token`.
pub fn terminal_prompts_disabled() -> bool {
    std::env::var("GIT_TERMINAL_PROMPT").is_ok_and(|v| matches!(v.as_str(), "0" | "false" | "no" | "off"))
}

/// The terminal for prompts, apart from stdin (which carries protocol input):
/// `/dev/tty`, or the console (`CONIN$` / `CONOUT$`) on Windows.
// Used by `ask`.
fn open_terminal() -> std::io::Result<(fs::File, fs::File)> {
    if cfg!(windows) {
        Ok((fs::File::open("CONIN$")?, fs::OpenOptions::new().write(true).open("CONOUT$")?))
    } else {
        let tty = fs::OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        Ok((tty.try_clone()?, tty))
    }
}

/// Parses `credential-store`'s options (`--file=<path>` / `--file <path>`), returning the
/// files it works on and the remaining words. Without `--file`: `~/.git-credentials` and
/// `$XDG_CONFIG_HOME/git/credentials` (or `~/.config/git/credentials`), like git.
// Used by `credential-store` and the built-in `store` helper.
pub fn parse_store_options(args: &[String]) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    let mut file: Option<PathBuf> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if let Some(path) = arg.strip_prefix("--file=") {
            file = Some(PathBuf::from(path));
        } else if arg == "--file" {
            i += 1;
            file = Some(PathBuf::from(args.get(i).ok_or_else(|| "option `file' requires a value".to_string())?));
        } else if arg.starts_with('-') && arg != "-" {
            return Err(format!("unknown option `{}'", arg.trim_start_matches('-')));
        } else {
            rest.push(arg.clone());
        }
        i += 1;
    }
    if let Some(file) = file {
        return Ok((vec![file], rest));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let xdg = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".config")));
    let mut files = Vec::new();
    if let Some(home) = home {
        files.push(home.join(".git-credentials"));
    }
    if let Some(xdg) = xdg {
        files.push(xdg.join("git").join("credentials"));
    }
    Ok((files, rest))
}

/// One `credential-store` action on `files`: `get` answers with the first stored login that
/// matches, `store` saves a complete login (replacing the same user's entry for that host),
/// `erase` removes matching logins from every file. Unknown actions do nothing, as in git.
// Used by `credential-store` and the built-in `store` helper.
pub fn store_action(files: &[PathBuf], action: &str, credential: &Credential) -> anyhow::Result<Option<Credential>> {
    match action {
        "get" => {
            for file in files {
                let Ok(text) = fs::read_to_string(file) else { continue };
                let found = text.lines().filter_map(Credential::from_url).find(|stored| credential.matches(stored, false));
                if let Some(stored) = found {
                    return Ok(Some(Credential { username: stored.username, password: stored.password, ..Default::default() }));
                }
            }
            Ok(None)
        }
        "store" => {
            if credential.protocol.is_none() || (credential.host.is_none() && credential.path.is_none()) || !credential.is_complete() {
                return Ok(None);
            }
            let target = files.iter().find(|f| f.exists()).or(files.first());
            if let Some(file) = target {
                let mut key = credential.clone();
                key.password = None;
                rewrite_store_file(file, &key, false, Some(&credential.to_store_line()))?;
            }
            Ok(None)
        }
        "erase" => {
            if credential.protocol.is_none() && credential.host.is_none() && credential.path.is_none() && credential.username.is_none() {
                return Ok(None);
            }
            for file in files.iter().filter(|f| f.exists()) {
                rewrite_store_file(file, credential, true, None)?;
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// Rewrites a credentials file without the lines matching `remove` (lines that are not
/// credential URLs are kept), with `first` as the new first line. The file is replaced
/// through a `.lock` file and is readable by its owner only.
// Used by `store_action`.
fn rewrite_store_file(file: &std::path::Path, remove: &Credential, with_password: bool, first: Option<&str>) -> anyhow::Result<()> {
    let old = fs::read_to_string(file).unwrap_or_default();
    let mut lines: Vec<String> = first.map(str::to_string).into_iter().collect();
    for line in old.lines() {
        let matched = Credential::from_url(line).is_some_and(|stored| remove.matches(&stored, with_password));
        if !matched {
            lines.push(line.to_string());
        }
    }
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let lock = file.with_file_name(format!("{}.lock", file.file_name().unwrap_or_default().to_string_lossy()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut handle = options.open(&lock).map_err(|e| anyhow::anyhow!("fatal: unable to write credential store: {}", e))?;
    handle.write_all(text.as_bytes())?;
    drop(handle);
    fs::rename(&lock, file).map_err(|e| anyhow::anyhow!("fatal: unable to write credential store: {}", e))?;
    Ok(())
}

/// The credential a GitHub command got from `fill`, and whether it was typed, kept until
/// the first authenticated request says whether it works.
// Used by `set_pending`, `approve_pending` and `reject_pending`.
static PENDING: Mutex<Option<(Credential, bool)>> = Mutex::new(None);

/// Remembers the credential behind the token a GitHub command is about to use.
// Used by `resolve_github_token`.
pub fn set_pending(credential: Credential, typed: bool) {
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some((credential, typed));
    }
}

/// The first authenticated GitHub request succeeded: helpers store the login. A typed login
/// with no helper to keep it gets a hint, outside `--plain` / quiet output.
// Used by the GitHub client (`get_github_branch_commit`, `list_github_refs`, `get_github_default_branch`).
pub fn approve_pending() {
    let Some((credential, typed)) = PENDING.lock().ok().and_then(|mut p| p.take()) else { return };
    let _ = approve(&credential);
    if typed && configured_helpers().is_empty() && crate::ui::level() >= crate::ui::types::Verbosity::Normal {
        eprintln!("hint: no credential helper is set, so rgit will ask again next time");
        eprintln!("hint: `rgit config --global credential.helper store` remembers it in ~/.git-credentials");
    }
}

/// GitHub refused the token: helpers forget the login, so the next run asks again.
// Used by the GitHub client on 401 responses.
pub fn reject_pending() {
    if let Some((credential, _)) = PENDING.lock().ok().and_then(|mut p| p.take()) {
        let _ = reject(&credential);
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/remote/credential/engine.rs"]
mod tests;
