//! rgit's narration layer ("the git that explains itself", ideas file C.1 + C.10).
//!
//! Every state-changing command is wrapped: `begin` takes a snapshot of the repository before
//! it runs, and an exit hook compares it with the state afterwards and says, in plain words,
//! what actually happened (or why nothing did) and what to do next. All wording lives in this
//! file. Narration goes to stderr only, so stdout stays exactly as before.
//!
//! Levels: `quiet` (git-identical, nothing extra), `normal` (one-line notes, conflict reports,
//! warnings), `explain` (+ what it means), `teach` (+ the refs and index changed underneath).
//! Chosen by `rgit --plain|--explain|--teach <cmd>`, `RGIT_VERBOSITY`, or `rgit.verbosity`;
//! the default is `normal`, but only when stderr is a terminal, so scripts and parity tests
//! that pipe rgit's output see no change.

pub mod types;

pub mod narrate;

pub mod before;
pub mod report;

use crate::ui::{
    before::before_command,
    narrate::take_snapshot,
    report::{report, teach_changes},
    report::remote::report_clone,
    types::{RepoSnapshot, Verbosity},
};
use std::io::IsTerminal;
use std::sync::{Mutex, OnceLock};

static LEVEL: OnceLock<Verbosity> = OnceLock::new();
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static OUTCOME: Mutex<Option<bool>> = Mutex::new(None);

/// The command being narrated: its name, arguments and the state before it ran.
struct Pending {
    command: String,
    args: Vec<String>,
    before: Option<RepoSnapshot>,
    cwd: std::path::PathBuf,
}

/// Parses a verbosity name (`quiet`/`plain`, `normal`, `explain`/`verbose`, `teach`).
fn parse_level(value: &str) -> Option<Verbosity> {
    match value.trim().to_ascii_lowercase().as_str() {
        "quiet" | "plain" | "off" | "0" => Some(Verbosity::Quiet),
        "normal" | "1" => Some(Verbosity::Normal),
        "explain" | "verbose" | "2" => Some(Verbosity::Explain),
        "teach" | "3" => Some(Verbosity::Teach),
        _ => None,
    }
}

/// Decides the narration level once: a global flag wins, then `RGIT_VERBOSITY`, then
/// `rgit.verbosity`; without any of them, `normal` when stderr is a terminal (and this is not
/// an rgit started by rgit), `quiet` otherwise.
pub fn init(flag: Option<Verbosity>) {
    let explicit = flag
        .or_else(|| std::env::var("RGIT_VERBOSITY").ok().and_then(|v| parse_level(&v)))
        .or_else(|| crate::core::config::get_config_value("rgit.verbosity").ok().flatten().and_then(|v| parse_level(&v)));
    let level = explicit.unwrap_or_else(|| {
        if std::io::stderr().is_terminal() && std::env::var_os("RGIT_NESTED").is_none() {
            Verbosity::Normal
        } else {
            Verbosity::Quiet
        }
    });
    let _ = LEVEL.set(level);
}

/// The narration level (quiet until `init` ran).
pub fn level() -> Verbosity {
    *LEVEL.get().unwrap_or(&Verbosity::Quiet)
}

fn colors() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none() && (cfg!(not(windows)) || std::env::var_os("WT_SESSION").is_some() || std::env::var_os("TERM").is_some())
}

fn paint(code: &str, text: &str) -> String {
    if colors() { format!("\x1b[{}m{}\x1b[0m", code, text) } else { text.to_string() }
}

/// A one-line note about what happened (normal level).
pub fn note(text: &str) {
    if level() >= Verbosity::Normal {
        eprintln!("{} {}", paint("36", "rgit:"), text);
    }
}

/// An indented continuation line (normal level).
fn line(text: &str) {
    if level() >= Verbosity::Normal {
        eprintln!("      {}", text);
    }
}

/// Extra explanation, shown from the explain level up.
pub fn detail(text: &str) {
    if level() >= Verbosity::Explain {
        eprintln!("      {}", paint("2", text));
    }
}

/// A warning that might prevent a mistake (yellow, normal level).
pub fn warn(text: &str) {
    if level() >= Verbosity::Normal {
        eprintln!("{} {}", paint("33", "rgit: warning:"), text);
    }
}

/// Something that loses data or rewrites shared history (red, normal level).
pub fn danger(text: &str) {
    if level() >= Verbosity::Normal {
        eprintln!("{} {}", paint("31", "rgit:"), text);
    }
}

/// The exact next command(s) to run (normal level).
fn next(text: &str) {
    if level() >= Verbosity::Normal {
        eprintln!("{} {}", paint("32", "  next:"), text);
    }
}

/// Plumbing-level detail, shown only at the teach level.
fn teach(text: &str) {
    if level() >= Verbosity::Teach {
        eprintln!("{} {}", paint("35", "  under the hood:"), text);
    }
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

fn list(paths: &[String], max: usize) -> String {
    let mut out = paths.iter().take(max).cloned().collect::<Vec<_>>().join(", ");
    if paths.len() > max {
        out.push_str(&format!(" and {} more", paths.len() - max));
    }
    out
}

/// Commands whose effects rgit narrates, and whether their snapshot needs the working tree.
fn narrated(command: &str) -> Option<bool> {
    match command {
        "add" | "stage" | "commit" | "reset" | "restore" | "checkout" | "switch" | "stash" | "rm" | "clean" | "diff" => Some(true),
        "merge" | "pull" | "fetch" | "push" | "rebase" | "cherry-pick" | "revert" | "tag" | "branch" | "status" | "clone" | "log" => Some(false),
        _ => None,
    }
}

fn split_args() -> (String, Vec<String>) {
    let mut args = std::env::args().skip(1).skip_while(|a| a.starts_with('-'));
    let command = args.next().unwrap_or_default();
    (if command == "stage" { "add".into() } else { command }, args.collect())
}

/// Positional (non-option) arguments, skipping the values of options that take one.
fn positionals(args: &[String], value_flags: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    let mut after_dashdash = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if after_dashdash {
            out.push(a.clone());
        } else if a == "--" {
            after_dashdash = true;
        } else if a.starts_with('-') && a.len() > 1 {
            if value_flags.contains(&a.as_str()) {
                skip = true;
            }
        } else {
            out.push(a.clone());
        }
    }
    out
}

fn has(args: &[String], flags: &[&str]) -> bool {
    args.iter().any(|a| flags.contains(&a.as_str()) || flags.iter().any(|f| f.starts_with("--") && a.starts_with(&format!("{}=", f))))
}

unsafe extern "C" {
    fn atexit(callback: extern "C" fn()) -> std::os::raw::c_int;
}

extern "C" fn at_exit() {
    let _ = std::panic::catch_unwind(finish);
}

/// Called once the command returned normally (true) or with an error (false). Commands that
/// stop early go through `ui::exit`, which records the outcome from the exit code.
/// Whether git-style conflict hints should be left out because the narration will explain the
/// same stop in its own words (narrating at normal level or above). Piped output keeps them.
pub fn narration_replaces_hints() -> bool {
    level() >= Verbosity::Normal && PENDING.lock().map(|p| p.is_some()).unwrap_or(false)
}

/// Ends the process like `std::process::exit`, recording the outcome first so the exit hook
/// knows whether the command succeeded. Every early exit in rgit goes through here.
pub fn exit(code: i32) -> ! {
    if code != 0
        && let Some(note) = FAILURE_NOTE.lock().ok().and_then(|mut n| n.take())
    {
        eprintln!("{}", note);
    }
    set_outcome(code == 0);
    crate::workspace::undo::engine::finish_recording();
    // Report now instead of relying on the `atexit` hook: on Windows `std::process::exit` is
    // `ExitProcess`, which skips CRT atexit handlers. `finish` takes the pending report, so the
    // hook finds nothing left to do on platforms where it does run.
    let _ = std::panic::catch_unwind(finish);
    std::process::exit(code)
}

/// A note `exit` prints when the command fails from deep inside code it called (set by
/// `note_on_failure`).
static FAILURE_NOTE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Sets (Some) or clears (None) a note printed to stderr if the process then exits non-zero,
/// for commands that call into code which stops the process itself (the rebase engine on a
/// conflict).
// Used by `fixup`.
pub fn note_on_failure(note: Option<String>) {
    if let Ok(mut slot) = FAILURE_NOTE.lock() {
        *slot = note;
    }
}

/// Prints `message` to stderr with a newline and ends the process with `code`. Commands use it
/// to stop with git's exact error text (which usually starts with `fatal:` or `error:` itself).
// Used by `merge`, `rebase`, `commit`, `tag`, `log`, `show-branch`, `update-index`, `interpret-trailers` and `fmt-merge-msg`.
pub fn die(code: i32, message: &str) -> ! {
    eprintln!("{}", crate::core::color::paint_hints(message));
    exit(code)
}

/// Prints `text` and a newline to stderr, its `hint:` lines in yellow on a terminal as git
/// prints advice.
// Used wherever rgit prints git's `hint:` lines.
pub fn eprintln_hints(text: &str) {
    eprintln!("{}", crate::core::color::paint_hints(text));
}

/// Prints `fatal: <message>` to stderr and ends the process with status 128, like git's `die()`.
// Used by `update-ref` and `branch`.
pub fn die_fatal(message: impl AsRef<str>) -> ! {
    eprintln!("fatal: {}", crate::core::color::paint_hints(message.as_ref()));
    exit(128)
}

/// Prints `message` to stderr exactly as given (no newline added) and ends the process with
/// `code`; for messages that already carry their own line breaks, such as usage texts.
// Used by `submodule`, `symbolic-ref` and `update-ref`.
pub fn die_raw(code: i32, message: &str) -> ! {
    eprint!("{}", crate::core::color::paint_hints(message));
    exit(code)
}

pub fn set_outcome(ok: bool) {
    if let Ok(mut o) = OUTCOME.lock() {
        *o = Some(ok);
    }
}

/// Starts narrating the current command: takes the "before" snapshot, prints warnings that
/// must come before the command acts, and registers the exit hook that reports afterwards.
pub fn begin() {
    if level() == Verbosity::Quiet || !crate::core::repo::exists() && split_args().0 != "clone" {
        return;
    }
    let (command, args) = split_args();
    let Some(with_worktree) = narrated(&command) else { return };
    // rgit processes started by this one (submodules, hooks, for-each-repo) stay quiet.
    unsafe { std::env::set_var("RGIT_NESTED", "1") };
    let before = if command == "clone" { None } else { Some(take_snapshot(with_worktree)) };
    if let Some(b) = &before {
        before_command(&command, &args, b);
    }
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(Pending { command, args, before, cwd: std::env::current_dir().unwrap_or_default() });
    }
    unsafe {
        atexit(at_exit);
    }
}

fn finish() {
    let Some(p) = PENDING.lock().ok().and_then(|mut g| g.take()) else { return };
    let _ = std::env::set_current_dir(&p.cwd);
    let outcome = OUTCOME.lock().ok().and_then(|o| *o);
    if p.command == "clone" {
        if outcome == Some(true) {
            report_clone(&p.args);
        }
        return;
    }
    let Some(before) = p.before else { return };
    let after = take_snapshot(narrated(&p.command).unwrap_or(false));
    report(&p.command, &p.args, &before, &after, outcome);
    teach_changes(&before, &after);
}

// ----------------------------------------------------------------------------------------
// Before the command runs
// ----------------------------------------------------------------------------------------
