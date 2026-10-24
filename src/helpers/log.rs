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
use crate::objects::{CommitSignature, DiffStatEntry, LogDateMode, LogDecorate, LogFormat, LogOptions, ParsedCommit};
use crate::refs;

/// Parses `log` / `show` arguments. Display options (`--pretty`, `--format`, `--oneline`,
/// `--date`, `--decorate`, `-p`, `--stat`, `--shortstat`, `--name-only`, `--name-status`,
/// `-s`) are taken here; everything else is a revision-walk option handed to the `rev-list`
/// parser. For `log`, a missing revision means `HEAD`; for `show` (`show_mode`), positional
/// revisions are collected in `objects` instead of walked, and `-p` is on by default.
/// Decoration defaults to `log.decorate`, or to short names when output is a terminal.
// Used by `log` and `show`.
pub fn parse_log_args(args: &[String], show_mode: bool) -> anyhow::Result<LogOptions> {
    let mut opts = LogOptions { patch: show_mode, ..Default::default() };
    let mut decorate: Option<LogDecorate> = None;
    let mut rev_args: Vec<String> = Vec::new();
    let mut abbrev_commit = false;
    let mut explicit_patch = false;
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
            }
            "--stat" => opts.stat = true,
            "--shortstat" => opts.shortstat = true,
            "--name-only" => opts.name_only = true,
            "--name-status" => opts.name_status = true,
            "--oneline" => {
                opts.format = LogFormat::Oneline;
                abbrev_commit = true;
            }
            "--abbrev-commit" => abbrev_commit = true,
            "--no-abbrev-commit" => abbrev_commit = false,
            "--decorate" => decorate = Some(LogDecorate::Short),
            "--no-decorate" => decorate = Some(LogDecorate::No),
            "--color" | "--no-color" => {}
            "--graph" => anyhow::bail!("fatal: --graph is not supported by rgit yet"),
            "--follow" => anyhow::bail!("fatal: --follow is not supported by rgit yet"),
            _ if arg.starts_with("--decorate=") => {
                decorate = Some(parse_decorate(&arg["--decorate=".len()..])?);
            }
            _ if arg.starts_with("--color=") => {}
            _ if arg == "--date" || arg.starts_with("--date=") => {
                opts.date = parse_date_mode(&value(arg, args, &mut i, "--date")?)?;
            }
            _ if arg == "--pretty" || arg.starts_with("--pretty=") => {
                let v = if arg == "--pretty" { "medium".to_string() } else { value(arg, args, &mut i, "--pretty")? };
                opts.format = parse_pretty(&v, false)?;
                abbrev_commit = abbrev_commit || opts.format == LogFormat::Oneline;
            }
            _ if arg == "--format" || arg.starts_with("--format=") => {
                opts.format = parse_pretty(&value(arg, args, &mut i, "--format")?, true)?;
            }
            _ if show_mode && !arg.starts_with('-') => opts.objects.push(arg.to_string()),
            _ => rev_args.push(arg.to_string()),
        }
        i += 1;
    }

    opts.revs = parse_revision_args(&rev_args)?;
    opts.revs.abbrev_commit = opts.revs.abbrev_commit || abbrev_commit;
    if !show_mode && opts.revs.revs.is_empty() && !(opts.revs.all || opts.revs.branches || opts.revs.tags || opts.revs.remotes) {
        opts.revs.revs.push("HEAD".to_string());
    }
    if show_mode && opts.objects.is_empty() {
        opts.objects.push("HEAD".to_string());
    }
    // As in git, asking `show` for another diff format replaces its default patch.
    if show_mode && !explicit_patch && (opts.stat || opts.shortstat || opts.name_only || opts.name_status) {
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

fn indented_message(message: &str) -> String {
    let mut out = String::new();
    for line in message.trim_start_matches('\n').trim_end().lines() {
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

    match &opts.format {
        LogFormat::Oneline => format!("{}{} {}\n", hash, decor, split_commit_message(&commit.message).0),
        LogFormat::Short => format!("commit {}{}\n{}Author: {}\n\n    {}\n", hash, decor, merge_line, signature_line(&commit.author), split_commit_message(&commit.message).0),
        LogFormat::Medium => format!(
            "commit {}{}\n{}Author: {}\nDate:   {}\n\n{}",
            hash,
            decor,
            merge_line,
            signature_line(&commit.author),
            date(&commit.author),
            indented_message(&commit.message)
        ),
        LogFormat::Full => format!(
            "commit {}{}\n{}Author: {}\nCommit: {}\n\n{}",
            hash,
            decor,
            merge_line,
            signature_line(&commit.author),
            signature_line(&commit.committer),
            indented_message(&commit.message)
        ),
        LogFormat::Fuller => format!(
            "commit {}{}\n{}Author:     {}\nAuthorDate: {}\nCommit:     {}\nCommitDate: {}\n\n{}",
            hash,
            decor,
            merge_line,
            signature_line(&commit.author),
            date(&commit.author),
            signature_line(&commit.committer),
            date(&commit.committer),
            indented_message(&commit.message)
        ),
        LogFormat::Raw => {
            let mut out = format!("commit {}{}\ntree {}\n", commit.hash, decor, commit.tree);
            for parent in &commit.parents {
                out.push_str(&format!("parent {}\n", parent));
            }
            let raw = |sig: &CommitSignature| format!("{} {} {}", signature_line(sig), sig.timestamp, sig.tz);
            out.push_str(&format!("author {}\ncommitter {}\n\n{}", raw(&commit.author), raw(&commit.committer), indented_message(&commit.message)));
            out
        }
        LogFormat::Template { template, terminator } => {
            let text = expand_rev_list_format(&substitute_decorations(template, &labels), commit, &commit.parents);
            if *terminator { format!("{}\n", text) } else { text }
        }
    }
}

/// Builds the diff output `log` prints after a commit: `--name-only` / `--name-status`
/// (which take precedence), else `--stat` / `--shortstat` and/or the patch, against the first
/// parent (the empty tree for a root commit), limited to the path arguments. Merges get none,
/// like git without `--diff-merges`. With both a stat and a patch, the stat is followed by a
/// blank line and the patch.
// Used by `log` and `show`.
pub fn commit_diff_output(commit: &ParsedCommit, opts: &LogOptions) -> anyhow::Result<String> {
    let wants_diff = opts.patch || opts.stat || opts.shortstat || opts.name_only || opts.name_status;
    if !wants_diff || commit.parents.len() > 1 {
        return Ok(String::new());
    }
    let specs = normalize_pathspecs(&opts.revs.paths);
    let in_scope = |path: &str| specs.is_empty() || matches_ls_files_pathspec(path, &specs);

    if opts.name_only || opts.name_status {
        let (mut old, mut new) = (BTreeMap::new(), BTreeMap::new());
        if let Some(parent) = commit.parents.first() {
            flatten_tree(&parse_commit_object(parent)?.tree, "", &mut old)?;
        }
        flatten_tree(&commit.tree, "", &mut new)?;
        let paths: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
        let mut out = String::new();
        for path in paths.into_iter().filter(|p| in_scope(p)) {
            let status = match (old.get(path), new.get(path)) {
                (None, Some(_)) => 'A',
                (Some(_), None) => 'D',
                (Some(a), Some(b)) if a.1 & 0o170000 != b.1 & 0o170000 => 'T',
                (Some(a), Some(b)) if a != b => 'M',
                _ => continue,
            };
            if opts.name_only {
                out.push_str(&format!("{}\n", path));
            } else {
                out.push_str(&format!("{}\t{}\n", status, path));
            }
        }
        return Ok(out);
    }

    let diffs: Vec<_> = diff_commit_against_parent(commit)?.into_iter().filter(|d| in_scope(&d.stat.name)).collect();
    if diffs.is_empty() {
        return Ok(String::new());
    }
    let mut out = String::new();
    if opts.stat || opts.shortstat {
        let stats: Vec<DiffStatEntry> = diffs.iter().map(|d| d.stat.clone()).collect();
        let block = format_diffstat(&stats, 80);
        if opts.stat {
            out.push_str(&block);
        } else if let Some(summary) = block.lines().last() {
            out.push_str(summary);
            out.push('\n');
        }
        if opts.patch {
            out.push('\n');
        }
    }
    if opts.patch {
        for diff in &diffs {
            out.push_str(&diff.text);
        }
    }
    Ok(out)
}

/// Joins one entry's header and diff output the way git lays them out: one-line formats put
/// the diff straight after the line; others separate it with a blank line, or with `---` when a
/// stat precedes a patch.
// Used by `log` and `show`.
pub fn join_entry_and_diff(entry: &str, diff: &str, opts: &LogOptions) -> String {
    if diff.is_empty() {
        return entry.to_string();
    }
    match &opts.format {
        LogFormat::Oneline => format!("{}{}", entry, diff),
        LogFormat::Template { .. } if !(opts.stat || opts.shortstat) || opts.patch => format!("{}\n{}", entry, diff),
        LogFormat::Template { .. } => format!("{}{}", entry, diff),
        _ if (opts.stat || opts.shortstat) && opts.patch && !(opts.name_only || opts.name_status) => format!("{}---\n{}", entry, diff),
        _ => format!("{}\n{}", entry, diff),
    }
}
