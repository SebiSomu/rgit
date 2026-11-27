//! Terminal colors as git does them: when to color (`--color`, `color.<command>`,
//! `color.ui`, a terminal on stdout), git's default color of each slot (overridable with
//! `color.<section>.<slot>`), git's color specification parser, and the diff colorizer that
//! `diff`, `show`, `log -p` and `stash show` share.

use std::collections::HashMap;
use std::io::IsTerminal;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// Ends a colored piece of text.
pub const RESET: &str = "\x1b[m";
pub const BOLD: &str = "\x1b[1m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const MAGENTA: &str = "\x1b[35m";
pub const CYAN: &str = "\x1b[36m";
pub const BOLD_RED: &str = "\x1b[1;31m";
pub const BOLD_GREEN: &str = "\x1b[1;32m";
pub const BOLD_YELLOW: &str = "\x1b[1;33m";
pub const BOLD_BLUE: &str = "\x1b[1;34m";
pub const BOLD_MAGENTA: &str = "\x1b[1;35m";
pub const BOLD_CYAN: &str = "\x1b[1;36m";
/// git's `color.diff.whitespace` default: a red background.
pub const RED_BACKGROUND: &str = "\x1b[41m";

/// `--color` / `--no-color` of the running command: 0 none, 1 always, 2 never.
static OVERRIDE: AtomicU8 = AtomicU8::new(0);

/// Sets what `--color[=<when>]` / `--no-color` asked for (`None` for `auto` or no flag).
// Used by the commands that take `--color` (log, show, diff, grep, branch, status).
pub fn set_override(forced: Option<bool>) {
    OVERRIDE.store(match forced {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    }, Ordering::Relaxed);
}

/// Reads a `--color[=<when>]` / `--no-color` argument: `Some(Some(on))` for `always`/`never`
/// (`--color` alone is `always`, as in git), `Some(None)` for `auto`, `None` if it isn't one.
// Used by the commands that take `--color`.
pub fn parse_color_flag(arg: &str) -> Option<Option<bool>> {
    match arg {
        "--color" => Some(Some(true)),
        "--no-color" => Some(Some(false)),
        _ => match arg.strip_prefix("--color=")? {
            "always" | "true" | "yes" | "on" => Some(Some(true)),
            "never" | "false" | "no" | "off" => Some(Some(false)),
            _ => Some(None),
        },
    }
}

/// Whether `command`'s output is colored, as git decides it: a `--color` flag, else
/// `color.<command>` (`color.diff` for log and show), else `color.ui`; `always` colors, `never`
/// or `false` doesn't, `auto` (the default) and `true` color when stdout is a terminal that
/// understands colors. `NO_COLOR` turns the `auto` case off, as most tools do.
// Used by every command that colors its output.
pub fn enabled(command: &str) -> bool {
    match OVERRIDE.load(Ordering::Relaxed) {
        1 => return true,
        2 => return false,
        _ => {}
    }
    let settings = color_settings();
    let setting = settings.get(&format!("color.{}", command.to_ascii_lowercase())).or_else(|| settings.get("color.ui")).map(|v| v.to_ascii_lowercase());
    match setting.as_deref() {
        Some("always") => true,
        Some("never") | Some("false") | Some("no") | Some("off") | Some("0") => false,
        _ => terminal_wants_color(),
    }
}

/// Whether messages on stderr are colored for `command` (`color.advice` for hints,
/// `color.push` for push errors): like `enabled`, but for stderr and without the `--color`
/// flag, which is about stdout.
// Used by `paint_hints` and `push`.
pub fn stderr_enabled(command: &str) -> bool {
    let settings = color_settings();
    let setting = settings.get(&format!("color.{}", command.to_ascii_lowercase())).or_else(|| settings.get("color.ui")).map(|v| v.to_ascii_lowercase());
    match setting.as_deref() {
        Some("always") => true,
        Some("never") | Some("false") | Some("no") | Some("off") | Some("0") => false,
        _ => {
            std::io::stderr().is_terminal()
                && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
                && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
                && console_supports_ansi(true)
        }
    }
}

/// `text` with each `hint:` line in yellow (`color.advice.hint`), as git prints its advice on
/// a terminal; unchanged when stderr is not colored.
// Used by `ui::eprintln_hints` and `ui::die`.
pub fn paint_hints(text: &str) -> String {
    if !text.contains("hint:") || !stderr_enabled("advice") {
        return text.to_string();
    }
    let color = slot("advice", "hint", YELLOW);
    text.split('\n').map(|line| if line.starts_with("hint:") { format!("{}{}{}", color, line, RESET) } else { line.to_string() }).collect::<Vec<_>>().join("\n")
}

/// stdout is a terminal that shows colors, and nobody asked for none.
// Used by `enabled`.
fn terminal_wants_color() -> bool {
    std::io::stdout().is_terminal()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
        && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
        && console_supports_ansi(false)
}

/// The escape sequence of a color slot: `color.<section>.<slot>` when set (git's syntax, such
/// as `bold blue` or `#ff8800`), else git's default `default`.
// Used by every command that colors its output.
pub fn slot(section: &str, name: &str, default: &'static str) -> String {
    let key = format!("color.{}.{}", section, name.to_ascii_lowercase());
    match color_settings().get(&key).map(|v| parse_color(v)) {
        Some(Ok(escape)) => escape,
        _ => default.to_string(),
    }
}

/// `text` in `color` (nothing added when `color` is empty).
// Used by every command that colors its output.
pub fn paint(text: &str, color: &str) -> String {
    if color.is_empty() || text.is_empty() {
        text.to_string()
    } else {
        format!("{}{}{}", color, text, RESET)
    }
}

/// Every `color.*` setting (keys lowercased; later files win), read once per run.
// Used by `enabled` and `slot`.
fn color_settings() -> &'static HashMap<String, String> {
    static SETTINGS: OnceLock<HashMap<String, String>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        crate::core::config::read_all_config_entries()
            .into_iter()
            .filter(|(key, _)| key.to_ascii_lowercase().starts_with("color."))
            .map(|(key, value)| (key.to_ascii_lowercase(), value))
            .collect()
    })
}

/// The colors of a diff, from `color.diff.<slot>` or git's defaults.
// Used by `colorize_diff`.
struct DiffColors {
    meta: String,
    frag: String,
    func: String,
    old: String,
    new: String,
    context: String,
    whitespace: String,
}

impl DiffColors {
    fn load() -> DiffColors {
        DiffColors {
            meta: slot("diff", "meta", BOLD),
            frag: slot("diff", "frag", CYAN),
            func: slot("diff", "func", ""),
            old: slot("diff", "old", RED),
            new: slot("diff", "new", GREEN),
            context: slot("diff", "context", ""),
            whitespace: slot("diff", "whitespace", RED_BACKGROUND),
        }
    }
}

/// Colors text that contains diffs, as git does: file headers (`diff --git`, `index`, `---`,
/// `+++`, modes, renames...) bold, `@@ ... @@` cyan with the function name after it plain,
/// removed lines red, added lines green with trailing whitespace on a red background (git's
/// default whitespace check), and `--stat` lines with their `+` green and `-` red. Hunks are
/// followed by their line counts, so commit messages around them (indented like context
/// lines) are never taken for diff lines. Text outside diffs is returned as it was.
// Used by `diff`, `show`, `log -p`, `stash show` and `add -p`.
pub fn colorize_diff(text: &str) -> String {
    let colors = DiffColors::load();
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    // Lines left in the current hunk (old side, new side), or the combined-diff column count.
    let mut old_left = 0usize;
    let mut new_left = 0usize;
    let mut combined: Option<usize> = None;
    let mut in_header = false;
    // git checks for blank lines added at the end of a file only when there was a file before.
    let mut new_file = false;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        let line = *line;
        let (body, newline) = match line.strip_suffix('\n') {
            Some(body) => (body, "\n"),
            None => (line, ""),
        };
        if let Some(columns) = combined {
            let prefix: String = body.chars().take(columns).collect();
            if prefix.len() == columns && prefix.chars().all(|c| matches!(c, ' ' | '+' | '-')) && !body.starts_with("diff ") {
                let color = if prefix.contains('+') { &colors.new } else if prefix.contains('-') { &colors.old } else { &colors.context };
                out.push_str(&wrap(body, color));
                out.push_str(newline);
                continue;
            }
            combined = None;
        }
        if old_left > 0 || new_left > 0 {
            let colored = match body.as_bytes().first() {
                Some(b'-') => {
                    old_left = old_left.saturating_sub(1);
                    Some(wrap(body, &colors.old))
                }
                Some(b'+') => {
                    let at_eof = !new_file && blank_until_end_of_file(&lines[index..], new_left);
                    new_left = new_left.saturating_sub(1);
                    // Blank lines added at the end of the file are a whitespace error as a whole.
                    Some(if at_eof { wrap(body, &colors.whitespace) } else { added_line(body, &colors) })
                }
                Some(b' ') | None => {
                    old_left = old_left.saturating_sub(1);
                    new_left = new_left.saturating_sub(1);
                    Some(wrap(body, &colors.context))
                }
                Some(b'\\') => Some(wrap(body, &colors.context)),
                _ => None,
            };
            if let Some(colored) = colored {
                out.push_str(&colored);
                out.push_str(newline);
                continue;
            }
            old_left = 0;
            new_left = 0;
        }
        if body.starts_with("diff --git ") || body.starts_with("diff --cc ") || body.starts_with("diff --combined ") {
            in_header = true;
            new_file = false;
            out.push_str(&paint(body, &colors.meta));
        } else if body.starts_with("@@") {
            in_header = false;
            out.push_str(&hunk_header(body, &colors));
            if let Some((old, new, columns)) = hunk_counts(body) {
                if columns > 1 {
                    combined = Some(columns);
                } else {
                    old_left = old;
                    new_left = new;
                }
            }
        } else if in_header && is_header_line(body) {
            new_file |= body == "--- /dev/null";
            out.push_str(&paint(body, &colors.meta));
        } else if let Some(colored) = stat_line(body, &colors) {
            in_header = false;
            out.push_str(&colored);
        } else {
            in_header = false;
            out.push_str(body);
        }
        out.push_str(newline);
    }
    out
}

/// Whether the added line starting `rest` and every line after it in the hunk (`new_left`
/// lines on the new side) are blank additions, so they end the file: git's `blank-at-eof`.
// Used by `colorize_diff`.
fn blank_until_end_of_file(rest: &[&str], mut new_left: usize) -> bool {
    for line in rest {
        if new_left == 0 {
            break;
        }
        let body = line.trim_end_matches('\n');
        match body.as_bytes().first() {
            Some(b'+') if body[1..].trim().is_empty() => new_left -= 1,
            Some(b'-') | Some(b'\\') => {}
            _ => return false,
        }
    }
    new_left == 0
}

/// An added line, with trailing spaces and tabs shown as git's whitespace error.
// Used by `colorize_diff`.
fn added_line(body: &str, colors: &DiffColors) -> String {
    // git paints the `+` on its own, then the text, then any trailing whitespace.
    let (sign, text) = body.split_at(1);
    let trimmed = text.trim_end_matches([' ', '\t']);
    let mut out = wrap(sign, &colors.new);
    if !trimmed.is_empty() {
        out.push_str(&wrap(trimmed, &colors.new));
    }
    if trimmed.len() < text.len() {
        out.push_str(&wrap(&text[trimmed.len()..], &colors.whitespace));
    }
    out
}

/// `text` between `color` and a reset, as git writes each piece of a diff line (the reset
/// comes even when the color is empty).
// Used by `colorize_diff`.
fn wrap(text: &str, color: &str) -> String {
    format!("{}{}{}", color, text, RESET)
}

/// `@@ -1,2 +1,3 @@ fn name`: the range part in the frag color, the function name in func's.
// Used by `colorize_diff`.
fn hunk_header(body: &str, colors: &DiffColors) -> String {
    let marker: String = body.chars().take_while(|c| *c == '@').collect();
    match body[marker.len()..].find(marker.as_str()) {
        Some(end) => {
            let split = marker.len() + end + marker.len();
            let rest = &body[split..];
            // ` <function>` after the ranges: the space plain, the name in the func color.
            match rest.strip_prefix(' ') {
                Some(name) => format!("{}{}{}", wrap(&body[..split], &colors.frag), wrap(" ", &colors.context), wrap(name, &colors.func)),
                None => format!("{}{}", wrap(&body[..split], &colors.frag), rest),
            }
        }
        None => paint(body, &colors.frag),
    }
}

/// The line counts of a hunk header: (old count, new count, parents) for `@@ -a,b +c,d @@`;
/// for a combined diff (`@@@ -a,b -c,d +e,f @@@`) the parent count is above 1.
// Used by `colorize_diff`.
fn hunk_counts(body: &str) -> Option<(usize, usize, usize)> {
    let at = body.chars().take_while(|c| *c == '@').count();
    let inner = body[at..].trim_start();
    let ranges: Vec<&str> = inner.split(' ').take_while(|r| r.starts_with('-') || r.starts_with('+')).collect();
    let count = |range: &str| -> Option<usize> {
        let numbers = &range[1..];
        Some(match numbers.split_once(',') {
            Some((_, n)) => n.parse().ok()?,
            None => 1,
        })
    };
    let olds: Vec<&&str> = ranges.iter().filter(|r| r.starts_with('-')).collect();
    let new = ranges.iter().find(|r| r.starts_with('+'))?;
    let old = olds.first().map(|r| count(r)).unwrap_or(Some(0))?;
    Some((old, count(new)?, at - 1))
}

/// Lines of a file header after `diff --git`.
// Used by `colorize_diff`.
fn is_header_line(body: &str) -> bool {
    [
        "index ", "--- ", "+++ ", "old mode ", "new mode ", "deleted file mode ", "new file mode ", "similarity index ",
        "dissimilarity index ", "rename from ", "rename to ", "copy from ", "copy to ", "Binary files ", "GIT binary patch",
        "mode ", "deleted file", "new file",
    ]
    .iter()
    .any(|p| body.starts_with(p))
}

/// A `--stat` line (` path | 12 ++++---`) with its `+` green and `-` red.
// Used by `colorize_diff`.
fn stat_line(body: &str, colors: &DiffColors) -> Option<String> {
    if !body.starts_with(' ') {
        return None;
    }
    let bar = body.rfind(" | ")?;
    let after = &body[bar + 3..];
    let count_end = after.find(|c: char| !c.is_ascii_digit() && c != ' ').unwrap_or(after.len());
    let (count, graph) = after.split_at(count_end);
    if count.trim().is_empty() || !graph.chars().all(|c| c == '+' || c == '-') || graph.is_empty() {
        return None;
    }
    let plus: String = graph.chars().filter(|c| *c == '+').collect();
    let minus: String = graph.chars().filter(|c| *c == '-').collect();
    Some(format!("{} | {}{}{}", &body[..bar], count, paint(&plus, &colors.new), paint(&minus, &colors.old)))
}

/// Whether the console shows ANSI colors: always outside Windows; on Windows once the console
/// accepted virtual-terminal processing (Windows 10 and later).
// Used by `terminal_wants_color`.
fn console_supports_ansi(stderr: bool) -> bool {
    #[cfg(windows)]
    {
        static ENABLED: OnceLock<(bool, bool)> = OnceLock::new();
        let (stdout_ok, stderr_ok) = *ENABLED.get_or_init(windows_console::enable_virtual_terminal);
        if stderr { stderr_ok } else { stdout_ok }
    }
    #[cfg(not(windows))]
    {
        let _ = stderr;
        true
    }
}

#[cfg(windows)]
mod windows_console {
    use std::ffi::c_void;

    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(handle: u32) -> *mut c_void;
        fn GetConsoleMode(console: *mut c_void, mode: *mut u32) -> i32;
        fn SetConsoleMode(console: *mut c_void, mode: u32) -> i32;
    }

    /// Asks the Windows console to interpret ANSI escape sequences (needed by `cmd.exe` and
    /// classic PowerShell windows; Windows Terminal already does). Says, for stdout and for
    /// stderr, whether that console accepted.
    pub fn enable_virtual_terminal() -> (bool, bool) {
        let (mut stdout_ok, mut stderr_ok) = (false, false);
        for (which, is_stdout) in [(STD_OUTPUT_HANDLE, true), (STD_ERROR_HANDLE, false)] {
            // SAFETY: plain Win32 calls on the process's own standard handles; `mode` is a
            // valid u32 for GetConsoleMode to write.
            unsafe {
                let handle = GetStdHandle(which);
                let mut mode: u32 = 0;
                if handle.is_null() || GetConsoleMode(handle, &mut mode) == 0 {
                    continue;
                }
                let ok = mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0 || SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0;
                if is_stdout {
                    stdout_ok = ok;
                } else {
                    stderr_ok = ok;
                }
            }
        }
        (stdout_ok, stderr_ok)
    }
}

/// Two hex digits as a byte (`ff` -> 255).
// Used by `parse_color`.
fn hex_pair(bytes: &[u8]) -> Option<u8> {
    let h = |c: u8| (c as char).to_digit(16);
    Some((h(*bytes.first()?)? * 16 + h(*bytes.get(1)?)?) as u8)
}

/// Turns a git color specification (`red`, `bold blue`, `#ff0000 ul`, `reset`, `223`...) into
/// its ANSI escape sequence, like git's `color_parse`; `Err` for invalid values.
// Used by `for-each-ref` (`%(color:...)`), `color.<slot>` settings and `%C(...)` in log formats.
pub(crate) fn parse_color(value: &str) -> Result<String, ()> {
    #[derive(Clone, Copy, PartialEq)]
    enum C {
        Unspecified,
        Normal,
        Ansi(u8),
        C256(u8),
        Rgb(u8, u8, u8),
    }
    let parse_one = |word: &str| -> Option<C> {
        let lower = word.to_ascii_lowercase();
        if lower == "normal" {
            return Some(C::Normal);
        }
        if word.len() == 7 && word.starts_with('#') {
            if let (Some(r), Some(g), Some(b)) = (hex_pair(&word.as_bytes()[1..]), hex_pair(&word.as_bytes()[3..]), hex_pair(&word.as_bytes()[5..])) {
                return Some(C::Rgb(r, g, b));
            }
        }
        if lower == "default" {
            return Some(C::Ansi(39));
        }
        let names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
        let (offset, base) = match lower.strip_prefix("bright") {
            Some(rest) => (90, rest),
            None => (30, lower.as_str()),
        };
        if let Some(i) = names.iter().position(|n| *n == base) {
            return Some(C::Ansi(offset + i as u8));
        }
        let n: i64 = word.parse().ok()?;
        match n {
            -1 => Some(C::Normal),
            0..=7 => Some(C::Ansi(30 + n as u8)),
            8..=15 => Some(C::Ansi(90 + n as u8 - 8)),
            16..=255 => Some(C::C256(n as u8)),
            _ => None,
        }
    };
    let parse_attr = |word: &str| -> Option<u32> {
        let (negate, name) = match word.strip_prefix("no") {
            Some(rest) => (true, rest.strip_prefix('-').unwrap_or(rest)),
            None => (false, word),
        };
        let (val, neg) = match name {
            "bold" => (1, 22),
            "dim" => (2, 22),
            "italic" => (3, 23),
            "ul" => (4, 24),
            "blink" => (5, 25),
            "reverse" => (7, 27),
            "strike" => (9, 29),
            _ => return None,
        };
        Some(if negate { neg } else { val })
    };
    let output = |c: C, background: bool| -> String {
        let off = if background { 10 } else { 0 };
        match c {
            C::Unspecified | C::Normal => String::new(),
            C::Ansi(v) => format!("{}", v as u32 + off),
            C::C256(v) => format!("{};5;{}", 38 + off, v),
            C::Rgb(r, g, b) => format!("{};2;{};{};{}", 38 + off, r, g, b),
        }
    };

    let words: Vec<&str> = value.split(|c: char| c.is_ascii_whitespace()).filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return Ok(String::new());
    }
    let (mut has_reset, mut attr, mut fg, mut bg) = (false, 0u64, C::Unspecified, C::Unspecified);
    for word in words {
        if word.eq_ignore_ascii_case("reset") {
            has_reset = true;
            continue;
        }
        if let Some(c) = parse_one(word) {
            if fg == C::Unspecified {
                fg = c;
            } else if bg == C::Unspecified {
                bg = c;
            } else {
                return Err(());
            }
            continue;
        }
        match parse_attr(word) {
            Some(v) => attr |= 1 << v,
            None => return Err(()),
        }
    }
    let empty = |c: C| matches!(c, C::Unspecified | C::Normal);
    if !(has_reset || attr != 0 || !empty(fg) || !empty(bg)) {
        return Ok(String::new());
    }
    let mut out = String::from("\x1b[");
    let mut sep = usize::from(has_reset);
    for i in 0..64 {
        if attr & (1 << i) != 0 {
            if sep > 0 {
                out.push(';');
            }
            sep += 1;
            out.push_str(&i.to_string());
        }
    }
    for (c, background) in [(fg, false), (bg, true)] {
        if !empty(c) {
            if sep > 0 {
                out.push(';');
            }
            sep += 1;
            out.push_str(&output(c, background));
        }
    }
    out.push('m');
    Ok(out)
}
