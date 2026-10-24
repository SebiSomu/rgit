use std::io::{self, BufWriter, Write};
use anyhow::Result;

use crate::helpers::format_patch::parse_commit_object;
use crate::helpers::log::{collect_decorations, commit_diff_output, effective_decorate, format_log_date, format_log_entry, join_entry_and_diff, parse_log_args};
use crate::helpers::read_object;
use crate::helpers::rev_list::select_rev_list_commits;
use crate::helpers::rev_parse::resolve_rev;
use crate::objects::{LogFormat, LogOptions};
use crate::refs;

/// `rgit log [<options>] [<revision-range>] [[--] <path>...]`
///
/// Shows the commits reachable from the given revisions (default `HEAD`), newest first. The
/// walk is `rev-list`'s, so ranges, `--not`, `--all`, `-n`, `--skip`, `--since`/`--until`,
/// `--author`/`--grep`, `--first-parent`, `--merges`/`--no-merges`, `--reverse`,
/// `--topo-order`, and path limiting all behave as in git. Each commit is printed in a
/// `--pretty` format (`oneline`, `short`, `medium` (default), `full`, `fuller`, `raw`) or a
/// `--format` template (with `%d`/`%D` decorations), with dates per `--date`, ref names per
/// `--decorate` (on by default on a terminal), and optionally `-p`, `--stat`, `--shortstat`,
/// `--name-only` or `--name-status` against the first parent.
pub fn log(args: Vec<String>) -> Result<()> {
    let opts = parse_log_args(&args, false).unwrap_or_else(|e| exit_with(&e.to_string(), 128));
    if opts.revs.revs == ["HEAD"] && refs::resolve_head_commit()?.is_none() {
        let name = match refs::resolve_head()? {
            refs::HeadState::Branch(b) => b,
            refs::HeadState::Detached(_) => "HEAD".to_string(),
        };
        exit_with(&format!("fatal: your current branch '{}' does not have any commits yet", name), 128);
    }
    let (mut selected, _) = select_rev_list_commits(&opts.revs).unwrap_or_else(|e| exit_with(&e.to_string(), 128));
    if opts.revs.reverse {
        selected.reverse();
    }
    let decorations = collect_decorations(effective_decorate(&opts))?;

    let mut out = BufWriter::new(io::stdout().lock());
    for (i, walked) in selected.iter().enumerate() {
        let entry = format_log_entry(&walked.commit, &opts, &decorations);
        let diff = commit_diff_output(&walked.commit, &opts)?;
        let separator = if i > 0 { entry_separator(&opts) } else { "" };
        write_or_quit(&mut out, &format!("{}{}", separator, join_entry_and_diff(&entry, &diff, &opts)));
    }
    let _ = out.flush();
    Ok(())
}

/// `rgit show [<options>] [<object>...]`
///
/// Shows each object (default `HEAD`): a commit as `log -p` shows it (merges without a diff),
/// an annotated tag as its tagger, date and message followed by the object it points at, a
/// tree as the list of its entries, and a blob as its contents. Takes `log`'s display options
/// (`--pretty`/`--format`/`--oneline`, `--date`, `--decorate`, `--stat`, `--name-only`,
/// `--name-status`, `-s` to drop the diff) and `-- <path>...` to limit the diff.
pub fn show(args: Vec<String>) -> Result<()> {
    let opts = parse_log_args(&args, true).unwrap_or_else(|e| exit_with(&e.to_string(), 128));
    let decorations = collect_decorations(effective_decorate(&opts))?;
    let mut out = BufWriter::new(io::stdout().lock());
    let mut first = true;
    for name in &opts.objects {
        let hash = resolve_rev(name).unwrap_or_else(|_| {
            exit_with(
                &format!(
                    "fatal: ambiguous argument '{}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'rgit <command> [<revision>...] -- [<file>...]'",
                    name
                ),
                128,
            )
        });
        let text = show_object(&hash, name, &opts, &decorations, &mut first)?;
        write_or_quit(&mut out, &text);
    }
    let _ = out.flush();
    Ok(())
}

fn show_object(hash: &str, name: &str, opts: &LogOptions, decorations: &std::collections::HashMap<String, Vec<String>>, first: &mut bool) -> Result<String> {
    let (kind, content) = read_object(hash)?;
    let separator = if *first { "" } else { entry_separator(opts) };
    *first = false;
    Ok(match kind.as_str() {
        "commit" => {
            let commit = parse_commit_object(hash)?;
            let entry = format_log_entry(&commit, opts, decorations);
            let diff = commit_diff_output(&commit, opts)?;
            let mut text = format!("{}{}", separator, join_entry_and_diff(&entry, &diff, opts));
            let one_line = matches!(opts.format, LogFormat::Oneline | LogFormat::Template { .. });
            if commit.parents.len() > 1 && opts.patch && !one_line {
                text.push('\n');
            }
            text
        }
        "tag" => {
            let body = String::from_utf8_lossy(&content).into_owned();
            let (header, message) = body.split_once("\n\n").unwrap_or((body.as_str(), ""));
            let field = |key: &str| header.lines().find_map(|l| l.strip_prefix(key)).unwrap_or("").to_string();
            // git varies the tag header with the format: no tagger for one-line output, no date
            // for templates and short/full, and a blank line before the target only for
            // multi-line formats.
            let mut text = format!("{}tag {}\n", separator, field("tag "));
            let tagger = field("tagger ");
            if let Some((who, when)) = tagger.rsplit_once("> ") {
                let mut parts = when.split_whitespace();
                let timestamp: i64 = parts.next().and_then(|t| t.parse().ok()).unwrap_or(0);
                let tz = parts.next().unwrap_or("+0000");
                let date = format_log_date(timestamp, tz, opts.date);
                match opts.format {
                    LogFormat::Oneline => {}
                    LogFormat::Medium => text.push_str(&format!("Tagger: {}>\nDate:   {}\n", who, date)),
                    LogFormat::Fuller => text.push_str(&format!("Tagger:     {}>\nTaggerDate: {}\n", who, date)),
                    _ => text.push_str(&format!("Tagger: {}>\n", who)),
                }
            }
            text.push_str(&format!("\n{}\n", message.trim_end()));
            let mut inner_first = true;
            let target = field("object ");
            if !matches!(opts.format, LogFormat::Oneline | LogFormat::Template { .. }) {
                text.push('\n');
            }
            text.push_str(&show_object(&target, &target, opts, decorations, &mut inner_first)?);
            text
        }
        "tree" => {
            let mut text = format!("{}tree {}\n\n", separator, name);
            let mut pos = 0;
            while pos < content.len() {
                let space = content[pos..].iter().position(|&b| b == b' ').map(|p| p + pos).unwrap_or(content.len());
                let nul = content[space..].iter().position(|&b| b == 0).map(|p| p + space).unwrap_or(content.len());
                let mode = String::from_utf8_lossy(&content[pos..space]).into_owned();
                let entry_name = String::from_utf8_lossy(&content[(space + 1).min(nul)..nul]).into_owned();
                text.push_str(&entry_name);
                if mode == "40000" {
                    text.push('/');
                }
                text.push('\n');
                pos = nul + 21;
            }
            text
        }
        _ => format!("{}{}", separator, String::from_utf8_lossy(&content)),
    })
}

/// What git prints between two entries: nothing for one-line formats and `tformat:`
/// templates, a newline for `format:` templates and every multi-line format.
fn entry_separator(opts: &LogOptions) -> &'static str {
    match &opts.format {
        LogFormat::Oneline | LogFormat::Template { terminator: true, .. } => "",
        _ => "\n",
    }
}

fn write_or_quit(out: &mut impl Write, text: &str) {
    if let Err(e) = out.write_all(text.as_bytes()) {
        if e.kind() == io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        exit_with(&format!("fatal: {}", e), 128);
    }
}

fn exit_with(message: &str, code: i32) -> ! {
    eprintln!("{}", message);
    std::process::exit(code);
}
