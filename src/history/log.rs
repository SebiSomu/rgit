pub mod cli;
pub mod engine;
pub mod types;

use std::io::{self, BufWriter, Write};
use anyhow::Result;

use crate::core::commit::parse_commit_object;
use std::collections::HashSet;

use crate::history::follow::{follow_commit_pairs, render_follow_diff};
use crate::history::graph::{graph_new, graph_padding_line, graph_prefix_lines, graph_show_commit, graph_show_commit_msg, graph_update};
use crate::history::log::engine::{collect_decorations, commit_diff_output, commit_diff_output_width, effective_decorate, format_log_date, format_log_entry, join_entry_and_diff, parse_log_args};
use crate::core::object_store::read_object;
use crate::history::rev_list::engine::{rewrite_rev_list_parents, select_rev_list_commits};
use crate::core::rev_parse::resolve_rev;
use crate::history::{log::types::{LogFormat, LogOptions}, types::WalkedCommit};
use crate::core::refs;

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
///
/// `--graph` draws the commit graph to the left of every line (git's layout, including merges,
/// octopus merges and collapsing branch lines) and implies `--topo-order`. `--follow <path>`
/// walks the whole history and shows the commits that change that one path, switching to the
/// old name at the commit that renamed or copied it (shown as `R`/`C` with its similarity).
pub fn log(args: Vec<String>) -> Result<()> {
    let opts = parse_log_args(&args, false).unwrap_or_else(|e| crate::ui::die(128, &e.to_string()));
    if opts.revs.revs == ["HEAD"] && refs::resolve_head_commit()?.is_none() {
        let name = match refs::resolve_head()? {
            refs::HeadState::Branch(b) => b,
            refs::HeadState::Detached(_) => "HEAD".to_string(),
        };
        crate::ui::die(128, &format!("fatal: your current branch '{}' does not have any commits yet", name));
    }
    if opts.graph || opts.follow {
        return log_graph_or_follow(&opts);
    }
    let (mut selected, _) = select_rev_list_commits(&opts.revs).unwrap_or_else(|e| crate::ui::die(128, &e.to_string()));
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
    crate::diff::rename::warn_rename_limit("diff.renameLimit");
    Ok(())
}

/// `log --graph` and/or `log --follow`. The commits come from the full walk (with `--follow`,
/// not limited by the path, since the path changes along the way); `--skip` drops walked
/// commits before anything is looked at, and `-n` counts the commits actually shown, as in
/// git. Every commit handed out is fed to the graph, even one `--follow` then does not show,
/// so the graph marks such gaps with `...` exactly like git does.
fn log_graph_or_follow(opts: &LogOptions) -> Result<()> {
    let mut walk = opts.revs.clone();
    let limit = walk.max_count.take().unwrap_or(usize::MAX);
    let skip = std::mem::take(&mut walk.skip);
    if opts.follow {
        walk.paths.clear();
    }
    let (mut selected, state) = select_rev_list_commits(&walk).unwrap_or_else(|e| crate::ui::die(128, &e.to_string()));
    let interesting: HashSet<String> = selected.iter().map(|w| w.commit.hash.clone()).collect();
    let decorations = collect_decorations(effective_decorate(opts))?;

    // --reverse (never with --graph) shows the same commits oldest first; with --follow the
    // path is still followed newest first, so the names line up.
    let mut follow_path = opts.revs.paths.first().cloned().filter(|_| opts.follow);
    let mut chosen: Vec<(WalkedCommit, String)> = Vec::new();
    let mut graph = opts.graph.then(graph_new);
    let mut out = BufWriter::new(io::stdout().lock());
    let mut shown = 0;
    let mut prev_missing_newline = false;
    for walked in selected.drain(..).skip(skip) {
        if shown >= limit {
            break;
        }
        let commit = &walked.commit;
        if let Some(g) = graph.as_mut() {
            let rewritten = if walk.paths.is_empty() { commit.parents.clone() } else { rewrite_rev_list_parents(&commit.parents, &state, walk.first_parent) };
            let mut parents: Vec<String> = Vec::new();
            for (n, parent) in rewritten.into_iter().enumerate() {
                if walk.first_parent && n > 0 {
                    break;
                }
                if interesting.contains(&parent) && !parents.contains(&parent) {
                    parents.push(parent);
                }
            }
            let mark = if walk.left_right { if walked.left { "<" } else { ">" } } else { "*" };
            graph_update(g, &commit.hash, mark, parents);
        }

        let stat_width = graph.as_ref().map(|g| 80usize.saturating_sub(g.width.max(2 * g.new_columns.len() as i32) as usize)).unwrap_or(80);
        let diff = match follow_path.as_mut() {
            Some(path) => {
                // Like git without --diff-merges, merges have no diff and so are not shown.
                if commit.parents.len() > 1 {
                    continue;
                }
                let (pairs, renamed_from) = follow_commit_pairs(commit, path)?;
                if pairs.is_empty() {
                    continue;
                }
                if let Some(old) = renamed_from {
                    *path = old;
                }
                render_follow_diff(&pairs, opts, stat_width)?
            }
            None => commit_diff_output_width(commit, opts, stat_width)?,
        };
        shown += 1;

        let entry = format_log_entry(commit, opts, &decorations);
        let Some(g) = graph.as_mut() else {
            chosen.push((walked, join_entry_and_diff(&entry, &diff, opts)));
            continue;
        };
        let mut text = String::new();
        if shown > 1 && !entry_separator(opts).is_empty() {
            // A blank line between entries keeps the branch lines going (unless the previous
            // entry did not end its last line, like `format:` templates).
            if !prev_missing_newline {
                text.push_str(&graph_padding_line(g));
            }
            text.push('\n');
        }
        text.push_str(&graph_show_commit(g));
        let full = join_entry_and_diff(&entry, &diff, opts);
        text.push_str(&graph_show_commit_msg(g, &entry));
        text.push_str(&graph_prefix_lines(g, &full[entry.len()..]));
        prev_missing_newline = !entry.ends_with('\n');
        write_or_quit(&mut out, &text);
    }

    if opts.revs.reverse {
        chosen.reverse();
    }
    for (i, (_, text)) in chosen.iter().enumerate() {
        let separator = if i > 0 { entry_separator(opts) } else { "" };
        write_or_quit(&mut out, &format!("{}{}", separator, text));
    }
    let _ = out.flush();
    crate::diff::rename::warn_rename_limit("diff.renameLimit");
    Ok(())
}

/// `rgit whatchanged [<options>] [<revision-range>] [[--] <path>...]`
///
/// `log` in the form git had before `log` learned to show diffs: every commit is followed
/// by the raw list of what it changed, one `:<old mode> <new mode> <old hash> <new hash>
/// <status>\t<path>` line per path (`A` added, `D` deleted, `M` modified, `T` type changed).
/// Commits that change nothing, and merges, are left out. It takes all of `log`'s options;
/// asking for another diff format (`-p`, `--stat`, `--name-only`, `--name-status`) replaces
/// the raw list.
pub fn whatchanged(args: Vec<String>) -> Result<()> {
    let mut opts = parse_log_args(&args, false).unwrap_or_else(|e| crate::ui::die(128, &e.to_string()));
    reject_log_only_options(&opts);
    if !(opts.patch || opts.stat || opts.shortstat || opts.name_only || opts.name_status) {
        opts.raw = true;
    }
    // `-n` and `--skip` count the commits that are shown, not the ones walked.
    let (limit, skip) = (opts.revs.max_count.take().unwrap_or(usize::MAX), std::mem::take(&mut opts.revs.skip));
    let (selected, _) = select_rev_list_commits(&opts.revs).unwrap_or_else(|e| crate::ui::die(128, &e.to_string()));
    let decorations = collect_decorations(effective_decorate(&opts))?;

    let mut shown = Vec::new();
    for walked in &selected {
        let diff = commit_diff_output(&walked.commit, &opts)?;
        if !diff.is_empty() {
            shown.push((&walked.commit, diff));
        }
    }
    let mut shown: Vec<_> = shown.into_iter().skip(skip).take(limit).collect();
    if opts.revs.reverse {
        shown.reverse();
    }

    let mut out = BufWriter::new(io::stdout().lock());
    for (i, (commit, diff)) in shown.iter().enumerate() {
        let entry = format_log_entry(commit, &opts, &decorations);
        let separator = if i > 0 { entry_separator(&opts) } else { "" };
        write_or_quit(&mut out, &format!("{}{}", separator, join_entry_and_diff(&entry, diff, &opts)));
    }
    let _ = out.flush();
    crate::diff::rename::warn_rename_limit("diff.renameLimit");
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
    let opts = parse_log_args(&args, true).unwrap_or_else(|e| crate::ui::die(128, &e.to_string()));
    reject_log_only_options(&opts);
    let decorations = collect_decorations(effective_decorate(&opts))?;
    let mut out = BufWriter::new(io::stdout().lock());
    let mut first = true;
    for name in &opts.objects {
        let hash = resolve_rev(name).unwrap_or_else(|_| {
            crate::ui::die(128, &format!(
                    "fatal: ambiguous argument '{}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'rgit <command> [<revision>...] -- [<file>...]'",
                    name
                ))
        });
        let text = show_object(&hash, name, &opts, &decorations, &mut first)?;
        write_or_quit(&mut out, &text);
    }
    let _ = out.flush();
    crate::diff::rename::warn_rename_limit("diff.renameLimit");
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

/// `--graph` and `--follow` are implemented for `log` only.
fn reject_log_only_options(opts: &LogOptions) {
    if opts.graph || opts.follow {
        let option = if opts.graph { "--graph" } else { "--follow" };
        crate::ui::die(128, &format!("fatal: {} is only supported by 'rgit log' for now", option));
    }
}

fn write_or_quit(out: &mut impl Write, text: &str) {
    if let Err(e) = out.write_all(text.as_bytes()) {
        if e.kind() == io::ErrorKind::BrokenPipe {
            crate::ui::exit(0);
        }
        crate::ui::die(128, &format!("fatal: {}", e));
    }
}

