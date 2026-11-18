use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::IsTerminal;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::helpers::commit::format_commit_date;
use crate::helpers::config::get_config_value;
use crate::helpers::flatten_tree;
use crate::helpers::format_patch::{civil_from_days, diff_commit_against_parent, format_diffstat, format_rfc2822_date, parse_commit_object, split_commit_message, tz_offset_seconds};
use crate::helpers::ls_files::{matches_ls_files_pathspec, normalize_pathspecs};
use crate::helpers::rev_list::{abbreviate_hash, expand_rev_list_format, list_all_refs, parse_revision_args};
use crate::helpers::rev_parse::peel_to_type;
use crate::objects::{DiffChange, DiffPlumbingOptions, DiffSide, CommitSignature, DiffStatEntry, LogDateMode, LogDecorate, LogFormat, LogOptions, ParsedCommit};
use crate::refs;

/// Parses `log` / `show` arguments. Display options (`--pretty`, `--format`, `--oneline`,
/// `--date`, `--decorate`, `-p`, `--stat`, `--shortstat`, `--name-only`, `--name-status`,
/// `-s`) are taken here; everything else is a revision-walk option handed to the `rev-list`
/// parser. For `log`, a missing revision means `HEAD`; for `show` (`show_mode`), positional
/// revisions are collected in `objects` instead of walked, and `-p` is on by default.
/// Decoration defaults to `log.decorate`, or to short names when output is a terminal.
// Used by `log` and `show`.
pub fn parse_log_args(args: &[String], show_mode: bool) -> anyhow::Result<LogOptions> {
    let mut opts = LogOptions { patch: show_mode, renames: crate::helpers::rename::default_rename_options("diff.renames"), ..Default::default() };
    let mut decorate: Option<LogDecorate> = None;
    let mut rev_args: Vec<String> = Vec::new();
    let mut abbrev_commit = false;
    let mut explicit_patch = false;
    let mut notes: Option<bool> = None;
    let mut pretty_given = false;
    let mut i = 0;

    let value = |arg: &str, args: &[String], i: &mut usize, name: &str| -> anyhow::Result<String> {
        if let Some(v) = arg.strip_prefix(&format!("{}=", name)) {
            return Ok(v.to_string());
        }
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| anyhow::anyhow!("fatal: option '{}' requires a value", name.trim_start_matches('-')))
    };

    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            rev_args.extend(args[i..].iter().cloned());
            break;
        }
        match arg {
            "-p" | "-u" | "--patch" => {
                opts.patch = true;
                explicit_patch = true;
            }
            "-s" | "--no-patch" => {
                opts.patch = false;
                opts.stat = false;
                opts.shortstat = false;
                opts.name_only = false;
                opts.name_status = false;
                opts.raw = false;
            }
            "--raw" => opts.raw = true,
            "--notes" | "--show-notes" => notes = Some(true),
            "--no-notes" => notes = Some(false),
            "--stat" => opts.stat = true,
            "--shortstat" => opts.shortstat = true,
            "--summary" => opts.summary = true,
            "--name-only" => opts.name_only = true,
            "--name-status" => opts.name_status = true,
            "--oneline" => {
                pretty_given = true;
                opts.format = LogFormat::Oneline;
                abbrev_commit = true;
            }
            "--abbrev-commit" => abbrev_commit = true,
            "--no-abbrev-commit" => abbrev_commit = false,
            "--decorate" => decorate = Some(LogDecorate::Short),
            "--no-decorate" => decorate = Some(LogDecorate::No),
            "--color" | "--no-color" => {}
            "--graph" => opts.graph = true,
            "--follow" => opts.follow = true,
            _ if crate::helpers::rename::parse_rename_option(arg, &mut opts.renames) => {}
            _ if arg.starts_with("--decorate=") => {
                decorate = Some(parse_decorate(&arg["--decorate=".len()..])?);
            }
            _ if arg.starts_with("--color=") => {}
            _ if arg == "--date" || arg.starts_with("--date=") => {
                opts.date = parse_date_mode(&value(arg, args, &mut i, "--date")?)?;
            }
            _ if arg == "--pretty" || arg.starts_with("--pretty=") => {
                pretty_given = true;
                let v = if arg == "--pretty" { "medium".to_string() } else { value(arg, args, &mut i, "--pretty")? };
                opts.format = parse_pretty(&v, false)?;
                abbrev_commit = abbrev_commit || opts.format == LogFormat::Oneline;
            }
            _ if arg == "--format" || arg.starts_with("--format=") => {
                pretty_given = true;
                opts.format = parse_pretty(&value(arg, args, &mut i, "--format")?, true)?;
            }
            _ if show_mode && !arg.starts_with('-') => opts.objects.push(arg.to_string()),
            _ => rev_args.push(arg.to_string()),
        }
        i += 1;
    }

    opts.show_notes = notes.unwrap_or(!pretty_given);
    opts.revs = parse_revision_args(&rev_args)?;
    if opts.graph {
        if opts.revs.reverse {
            anyhow::bail!("fatal: options '--reverse' and '--graph' cannot be used together");
        }
        // As in git, --graph implies --topo-order unless --date-order was asked for.
        if !opts.revs.date_order {
            opts.revs.topo_order = true;
        }
    }
    if opts.follow && opts.revs.paths.len() != 1 {
        anyhow::bail!("fatal: --follow requires exactly one pathspec");
    }
    opts.revs.abbrev_commit = opts.revs.abbrev_commit || abbrev_commit;
    if !show_mode && opts.revs.revs.is_empty() && !(opts.revs.all || opts.revs.branches || opts.revs.tags || opts.revs.remotes) {
        opts.revs.revs.push("HEAD".to_string());
    }
    if show_mode && opts.objects.is_empty() {
        opts.objects.push("HEAD".to_string());
    }
    // As in git, asking `show` for another diff format replaces its default patch.
    if show_mode && !explicit_patch && (opts.stat || opts.shortstat || opts.name_only || opts.name_status || opts.raw) {
        opts.patch = false;
    }
    opts.decorate = match decorate {
        Some(d) => d,
        None => match get_config_value("log.decorate")?.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("full") => LogDecorate::Full,
            Some("short") | Some("true") | Some("yes") | Some("1") => LogDecorate::Short,
            Some("false") | Some("no") | Some("0") => LogDecorate::No,
            _ if std::io::stdout().is_terminal() => LogDecorate::Short,
            _ => LogDecorate::No,
        },
    };
    Ok(opts)
}

fn parse_decorate(value: &str) -> anyhow::Result<LogDecorate> {
    Ok(match value {
        "short" => LogDecorate::Short,
        "full" => LogDecorate::Full,
        "no" => LogDecorate::No,
        "auto" if std::io::stdout().is_terminal() => LogDecorate::Short,
        "auto" => LogDecorate::No,
        _ => anyhow::bail!("fatal: invalid --decorate option: {}", value),
    })
}

fn parse_date_mode(value: &str) -> anyhow::Result<LogDateMode> {
    Ok(match value {
        "default" => LogDateMode::Default,
        "iso" | "iso8601" => LogDateMode::Iso,
        "iso-strict" | "iso8601-strict" => LogDateMode::IsoStrict,
        "short" => LogDateMode::Short,
        "raw" => LogDateMode::Raw,
        "unix" => LogDateMode::Unix,
        "rfc" | "rfc2822" => LogDateMode::Rfc,
        "relative" => LogDateMode::Relative,
        _ => anyhow::bail!("fatal: unsupported date format '{}' (rgit supports default, iso, iso-strict, short, raw, unix, rfc, relative)", value),
    })
}

/// Maps a `--pretty` / `--format` value to a layout: a built-in name, `format:` / `tformat:`,
/// or (for `--format`, or any value containing `%`) a `tformat:` template.
fn parse_pretty(value: &str, format_option: bool) -> anyhow::Result<LogFormat> {
    Ok(match value {
        "oneline" => LogFormat::Oneline,
        "short" => LogFormat::Short,
        "medium" => LogFormat::Medium,
        "full" => LogFormat::Full,
        "fuller" => LogFormat::Fuller,
        "raw" => LogFormat::Raw,
        _ => {
            if let Some(t) = value.strip_prefix("format:") {
                LogFormat::Template { template: t.to_string(), terminator: false }
            } else if let Some(t) = value.strip_prefix("tformat:") {
                LogFormat::Template { template: t.to_string(), terminator: true }
            } else if format_option || value.contains('%') {
                LogFormat::Template { template: value.to_string(), terminator: true }
            } else {
                anyhow::bail!("fatal: invalid --pretty format: {}", value)
            }
        }
    })
}

/// Formats a timestamp for `log --date=<mode>`, following git: `iso` is
/// `2026-01-02 11:30:00 +0200`, `iso-strict` `2026-01-02T11:30:00+02:00` (`Z` for UTC),
/// `short` the date only, `raw` `<unix> <tz>`, `rfc` RFC 2822, `relative` "3 days ago"
/// (with git's rounding and unit thresholds), and the default `Fri Jan 2 11:30:00 2026 +0200`.
// Used by `log` and `show`.
pub fn format_log_date(timestamp: i64, tz: &str, mode: LogDateMode) -> String {
    let local = timestamp + tz_offset_seconds(tz);
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let secs = local.rem_euclid(86_400);
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    match mode {
        LogDateMode::Default => format_commit_date(timestamp, tz),
        LogDateMode::Iso => format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}", year, month, day, h, m, s, tz),
        LogDateMode::IsoStrict => {
            let zone = if tz_offset_seconds(tz) == 0 { "Z".to_string() } else { format!("{}:{}", &tz[..3], &tz[3..]) };
            format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}", year, month, day, h, m, s, zone)
        }
        LogDateMode::Short => format!("{:04}-{:02}-{:02}", year, month, day),
        LogDateMode::Raw => format!("{} {}", timestamp, tz),
        LogDateMode::Unix => timestamp.to_string(),
        LogDateMode::Rfc => format_rfc2822_date(timestamp, tz),
        LogDateMode::Relative => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            relative_date(now - timestamp)
        }
    }
}

/// git's `show_date_relative`: seconds, minutes, hours, days, weeks, months, then years (with
/// months for the first five years), each rounded to the nearest unit.
fn relative_date(diff: i64) -> String {
    let unit = |n: i64, word: &str| format!("{} {}{} ago", n, word, if n == 1 { "" } else { "s" });
    if diff < 0 {
        return "in the future".to_string();
    }
    if diff < 90 {
        return unit(diff, "second");
    }
    let minutes = (diff + 30) / 60;
    if minutes < 90 {
        return unit(minutes, "minute");
    }
    let hours = (minutes + 30) / 60;
    if hours < 36 {
        return unit(hours, "hour");
    }
    let days = (hours + 12) / 24;
    if days < 14 {
        return unit(days, "day");
    }
    if days < 70 {
        return unit((days + 3) / 7, "week");
    }
    if days < 365 {
        return unit((days + 15) / 30, "month");
    }
    if days < 1825 {
        let total_months = (days * 12 * 2 + 365) / (365 * 2);
        let (years, months) = (total_months / 12, total_months % 12);
        let years_text = format!("{} year{}", years, if years == 1 { "" } else { "s" });
        return if months > 0 { format!("{}, {}", years_text, unit(months, "month")) } else { format!("{} ago", years_text) };
    }
    unit((days + 183) / 365, "year")
}

/// The decoration mode to load refs for: the one asked for, or short names when it is off
/// but a `--format` template uses `%d` / `%D` (git fills those in regardless).
// Used by `log` and `show`.
pub fn effective_decorate(opts: &LogOptions) -> LogDecorate {
    match &opts.format {
        LogFormat::Template { template, .. } if opts.decorate == LogDecorate::No && (template.contains("%d") || template.contains("%D")) => LogDecorate::Short,
        _ => opts.decorate,
    }
}

/// Collects the ref names to print next to each commit, keyed by commit hash, as git's
/// `--decorate` shows them: refs in reverse name order (annotated tags on the commit they
/// peel to, as `tag: <name>`), with HEAD first, either as `HEAD -> <branch>` in place of the
/// current branch or as plain `HEAD` when detached. `Full` keeps the `refs/...` names.
// Used by `log` and `show`.
pub fn collect_decorations(mode: LogDecorate) -> anyhow::Result<HashMap<String, Vec<String>>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    if mode == LogDecorate::No {
        return Ok(map);
    }
    let label = |name: &str| -> String {
        if mode == LogDecorate::Full {
            return if name.starts_with("refs/tags/") { format!("tag: {}", name) } else { name.to_string() };
        }
        if let Some(tag) = name.strip_prefix("refs/tags/") {
            format!("tag: {}", tag)
        } else {
            name.strip_prefix("refs/heads/").or_else(|| name.strip_prefix("refs/remotes/")).unwrap_or(name).to_string()
        }
    };
    let refs: BTreeMap<String, String> = list_all_refs();
    for (name, hash) in &refs {
        let target = peel_to_type(hash.clone(), "commit").unwrap_or_else(|_| hash.clone());
        map.entry(target).or_default().push(label(name));
    }
    for labels in map.values_mut() {
        labels.reverse();
    }
    match refs::resolve_head()? {
        refs::HeadState::Branch(branch) => {
            let full = format!("refs/heads/{}", branch);
            if let Some(hash) = refs.get(&full) {
                let labels = map.entry(hash.clone()).or_default();
                let own = label(&full);
                labels.retain(|l| *l != own);
                labels.insert(0, format!("HEAD -> {}", own));
            }
        }
        refs::HeadState::Detached(hash) => map.entry(hash).or_default().insert(0, "HEAD".to_string()),
    }
    Ok(map)
}

fn signature_line(sig: &CommitSignature) -> String {
    format!("{} <{}>", sig.name, sig.email)
}

/// Renders the message part of a multi-line log entry: a blank line and the message indented by
/// four spaces, or nothing at all for a commit without a message (git prints only the header).
// Used by `format_log_entry`.
fn message_block(message: &str) -> String {
    let body = message.trim_start_matches('\n').trim_end();
    if body.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n");
    for line in body.lines() {
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Expands `%d` (` (refs)`) and `%D` (`refs`) in a template, leaving `%%` and every other
/// placeholder for `expand_rev_list_format`.
fn substitute_decorations(template: &str, labels: &[String]) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('d') => {
                chars.next();
                if !labels.is_empty() {
                    out.push_str(&format!(" ({})", labels.join(", ")));
                }
            }
            Some('D') => {
                chars.next();
                out.push_str(&labels.join(", "));
            }
            Some('%') => {
                chars.next();
                out.push_str("%%");
            }
            _ => out.push('%'),
        }
    }
    out
}

/// Renders one commit's header and message in the chosen layout (without any diff output and
/// without the blank line git puts between entries).
// Used by `log` and `show`.
pub fn format_log_entry(commit: &ParsedCommit, opts: &LogOptions, decorations: &HashMap<String, Vec<String>>) -> String {
    let labels = decorations.get(&commit.hash).cloned().unwrap_or_default();
    let decor = if labels.is_empty() { String::new() } else { format!(" ({})", labels.join(", ")) };
    let hash = if opts.revs.abbrev_commit { abbreviate_hash(&commit.hash) } else { commit.hash.clone() };
    let date = |sig: &CommitSignature| format_log_date(sig.timestamp, &sig.tz, opts.date);
    let merge_line = if commit.parents.len() > 1 {
        let short: Vec<String> = commit.parents.iter().map(|p| abbreviate_hash(p)).collect();
        format!("Merge: {}\n", short.join(" "))
    } else {
        String::new()
    };

    let entry = match &opts.format {
        LogFormat::Oneline => format!("{}{} {}\n", hash, decor, split_commit_message(&commit.message).0),
        LogFormat::Short => format!("commit {}{}\n{}Author: {}\n{}", hash, decor, merge_line, signature_line(&commit.author), message_block(&split_commit_message(&commit.message).0)),
        LogFormat::Medium => format!(
            "commit {}{}\n{}Author: {}\nDate:   {}\n{}",
            hash,
            decor,
            merge_line,
            signature_line(&commit.author),
            date(&commit.author),
            message_block(&commit.message)
        ),
        LogFormat::Full => format!(
            "commit {}{}\n{}Author: {}\nCommit: {}\n{}",
            hash,
            decor,
            merge_line,
            signature_line(&commit.author),
            signature_line(&commit.committer),
            message_block(&commit.message)
        ),
        LogFormat::Fuller => format!(
            "commit {}{}\n{}Author:     {}\nAuthorDate: {}\nCommit:     {}\nCommitDate: {}\n{}",
            hash,
            decor,
            merge_line,
            signature_line(&commit.author),
            date(&commit.author),
            signature_line(&commit.committer),
            date(&commit.committer),
            message_block(&commit.message)
        ),
        LogFormat::Raw => {
            let mut out = format!("commit {}{}\ntree {}\n", commit.hash, decor, commit.tree);
            for parent in &commit.parents {
                out.push_str(&format!("parent {}\n", parent));
            }
            let raw = |sig: &CommitSignature| format!("{} {} {}", signature_line(sig), sig.timestamp, sig.tz);
            out.push_str(&format!("author {}\ncommitter {}\n{}", raw(&commit.author), raw(&commit.committer), message_block(&commit.message)));
            out
        }
        LogFormat::Template { template, terminator } => {
            let text = expand_rev_list_format(&substitute_decorations(template, &labels), commit, &commit.parents);
            if *terminator { format!("{}\n", text) } else { text }
        }
    };

    // git appends the commit's note to every multi-line format.
    let note = if opts.show_notes && !matches!(opts.format, LogFormat::Oneline | LogFormat::Template { .. }) { crate::helpers::notes::display_note(&commit.hash) } else { None };
    match note {
        Some(note) => {
            let lines: String = note.lines().map(|line| format!("    {}\n", line)).collect();
            format!("{}\nNotes:\n{}", entry, lines)
        }
        None => entry,
    }
}

/// Builds the diff output `log` prints after a commit: `--name-only` / `--name-status`
/// (which take precedence), else `--stat` / `--shortstat` and/or the patch, against the first
/// parent (the empty tree for a root commit), limited to the path arguments. Merges get none,
/// like git without `--diff-merges`. With both a stat and a patch, the stat is followed by a
/// blank line and the patch.
// Used by `log` and `show`.
pub fn commit_diff_output(commit: &ParsedCommit, opts: &LogOptions) -> anyhow::Result<String> {
    commit_diff_output_width(commit, opts, 80)
}

/// `commit_diff_output` with the `--stat` block laid out for `stat_width` columns (narrower
/// than 80 when `--graph` takes up the left of each line, as in git).
// Used by `log --graph` and `commit_diff_output`.
pub fn commit_diff_output_width(commit: &ParsedCommit, opts: &LogOptions, stat_width: usize) -> anyhow::Result<String> {
    let wants_diff = opts.patch || opts.stat || opts.shortstat || opts.summary || opts.name_only || opts.name_status || opts.raw;
    if !wants_diff || commit.parents.len() > 1 {
        return Ok(String::new());
    }
    let specs = normalize_pathspecs(&opts.revs.paths);
    let in_scope = |path: &str| specs.is_empty() || matches_ls_files_pathspec(path, &specs);

    // The changes against the first parent (the empty tree for a root commit), in path order,
    // limited to the paths, then rename/copy detection as configured.
    let (mut old, mut new) = (BTreeMap::new(), BTreeMap::new());
    if let Some(parent) = commit.parents.first() {
        flatten_tree(&parse_commit_object(parent)?.tree, "", &mut old)?;
    }
    flatten_tree(&commit.tree, "", &mut new)?;
    let side = |e: &([u8; 20], u32)| DiffSide { mode: e.1, hash: e.0, worktree: false };
    let paths: BTreeSet<&String> = old.keys().chain(new.keys()).filter(|p| in_scope(p)).collect();
    let mut changes = Vec::new();
    for path in paths {
        let (a, b) = (old.get(path), new.get(path));
        if a != b {
            changes.push(DiffChange { path: path.clone(), old: a.map(side), new: b.map(side), renamed_from: None, unmerged: false });
        }
    }
    if opts.renames.level > 0 {
        let mut start = if opts.renames.find_copies_harder { old.clone() } else { BTreeMap::new() };
        start.retain(|p, _| in_scope(p));
        let unmodified = crate::helpers::diff_plumbing::unmodified_files(&start, &changes);
        changes = crate::helpers::rename::detect_renames(changes, &opts.renames, &unmodified);
    }
    if changes.is_empty() {
        return Ok(String::new());
    }

    // `--name-only`, `--raw` and `--name-status` replace the stat and patch output.
    let names = opts.name_only || opts.name_status || opts.raw;
    let render = DiffPlumbingOptions {
        patch: opts.patch && !names,
        stat: opts.stat && !names,
        shortstat: opts.shortstat && !names,
        summary: opts.summary && !names,
        name_only: opts.name_only,
        name_status: opts.name_status && !opts.name_only && !opts.raw,
        raw: opts.raw && !opts.name_only,
        abbrev: Some(7),
        stat_width,
        ..Default::default()
    };
    crate::helpers::diff_plumbing::render_diff_changes(&changes, &render)
}

/// `log -p` does not print binary patches (only `format-patch` does): a `GIT binary patch`
/// section becomes git's `Binary files a/<old> and b/<new> differ` line, with the `index` line
/// abbreviated as for text diffs. Other diff text is returned unchanged.
// Used by `log`, `show` and `log --follow`.
pub fn binary_patch_as_notice(text: &str, old_path: &str, new_path: &str) -> String {
    let Some(start) = text.find("\nGIT binary patch\n") else { return text.to_string() };
    let head = &text[..start];
    let index_at = head.rfind("\nindex ").map(|i| i + 1).unwrap_or(0);
    let index_line = &head[index_at..];
    let rest = index_line.trim_start_matches("index ");
    let (range, suffix) = rest.split_once(' ').map(|(r, s)| (r, format!(" {}", s))).unwrap_or((rest, String::new()));
    let (old_hash, new_hash) = range.split_once("..").unwrap_or((range, range));
    let short = |h: &str| h.chars().take(7).collect::<String>();
    let old_name = if head.contains("\nnew file mode ") { "/dev/null".to_string() } else { format!("a/{}", old_path) };
    let new_name = if head.contains("\ndeleted file mode ") { "/dev/null".to_string() } else { format!("b/{}", new_path) };
    format!("{}index {}..{}{}\nBinary files {} and {} differ\n", &head[..index_at], short(old_hash), short(new_hash), suffix, old_name, new_name)
}

/// Joins one entry's header and diff output the way git lays them out: one-line formats put
/// the diff straight after the line; every other format (templates included) ends the entry
/// with a newline, written as `---` when a `--stat` precedes a patch.
// Used by `log` and `show`.
pub fn join_entry_and_diff(entry: &str, diff: &str, opts: &LogOptions) -> String {
    if diff.is_empty() {
        return entry.to_string();
    }
    let dashes = opts.stat && opts.patch && !(opts.name_only || opts.name_status);
    match &opts.format {
        LogFormat::Oneline => format!("{}{}", entry, diff),
        _ => format!("{}{}\n{}", entry, if dashes { "---" } else { "" }, diff),
    }
}
