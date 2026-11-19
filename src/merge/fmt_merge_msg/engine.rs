use std::collections::BTreeMap;
use crate::core::commit::is_reachable;
use crate::core::config::{author_identity, get_config_value, read_all_config_entries};
use crate::core::commit::split_commit_message;
use crate::core::object_store::read_object;
use crate::history::rev_list::engine::run_rev_list_walk;
use crate::core::rev_parse::peel_to_type;
use crate::references::symbolic_ref::engine::resolve_ref_unsafe;
use crate::core::wildmatch::{wildmatch, WM_PATHNAME};
use crate::history::rev_list::types::{RevListOptions, RevListRoots};
use crate::merge::fmt_merge_msg::types::{FmtMergeMsgOptions, MergeOrigin, MergeSource};

/// git's usage text for `fmt-merge-msg`, printed for `-h` and after option errors (exit 129).
pub const FMT_MERGE_MSG_USAGE: &str = "usage: rgit fmt-merge-msg [-m <message>] [--log[=<n>] | --no-log] [--file <file>]

    --[no-]log[=<n>]      populate log with at most <n> entries from shortlog
    -m, --[no-]message <text>
                          use <text> as start of message
    --[no-]into-name <name>
                          use <name> instead of the real target branch
    -F, --[no-]file <file>
                          file to read from
";

/// Number of shortlog entries `--log` shows without a value (git's `DEFAULT_MERGE_LOG_LEN`).
const DEFAULT_MERGE_LOG_LEN: usize = 20;

/// Parses `fmt-merge-msg` arguments like git's option parser: `--log[=<n>]` (and its old
/// name `--summary`), `--no-log`, `-m <text>`, `--into-name <name>` and `-F <file>`, with
/// values glued on or as the next argument. Without `--log`, `merge.log` (a boolean or a
/// count) decides. Errors carry the usage text and mean exit 129.
// Used by `fmt-merge-msg`.
pub fn parse_fmt_merge_msg_args(args: &[String]) -> anyhow::Result<FmtMergeMsgOptions> {
    let mut opts = FmtMergeMsgOptions::default();
    let mut shortlog_len: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        let mut next = |name: &str| -> anyhow::Result<String> {
            let v = args.get(i).cloned().ok_or_else(|| anyhow::anyhow!("error: {} requires a value\n", name))?;
            i += 1;
            Ok(v)
        };
        let count = |name: &str, value: &str| -> anyhow::Result<usize> {
            value.parse::<usize>().map_err(|_| anyhow::anyhow!("error: option `{}' expects a numerical value\n", name))
        };
        match arg {
            "--log" | "--summary" => shortlog_len = Some(DEFAULT_MERGE_LOG_LEN),
            "--no-log" | "--no-summary" => shortlog_len = Some(0),
            "-m" => opts.message = Some(next("switch `m'")?),
            "--message" => opts.message = Some(next("option `message'")?),
            "--no-message" => opts.message = None,
            "--into-name" => opts.into_name = Some(next("option `into-name'")?),
            "--no-into-name" => opts.into_name = None,
            "-F" => opts.file = Some(next("switch `F'")?),
            "--file" => opts.file = Some(next("option `file'")?),
            "--no-file" => opts.file = None,
            _ => {
                if let Some(v) = arg.strip_prefix("--log=") {
                    shortlog_len = Some(count("log", v)?);
                } else if let Some(v) = arg.strip_prefix("--summary=") {
                    shortlog_len = Some(count("summary", v)?);
                } else if let Some(v) = arg.strip_prefix("--message=") {
                    opts.message = Some(v.to_string());
                } else if let Some(v) = arg.strip_prefix("--into-name=") {
                    opts.into_name = Some(v.to_string());
                } else if let Some(v) = arg.strip_prefix("--file=") {
                    opts.file = Some(v.to_string());
                } else if let Some(v) = arg.strip_prefix("-m").filter(|v| !v.is_empty()) {
                    opts.message = Some(v.to_string());
                } else if let Some(v) = arg.strip_prefix("-F").filter(|v| !v.is_empty()) {
                    opts.file = Some(v.to_string());
                } else if let Some(name) = arg.strip_prefix("--") {
                    anyhow::bail!("error: unknown option `{}'\n{}", name, FMT_MERGE_MSG_USAGE);
                } else if arg.starts_with('-') && arg.len() > 1 {
                    anyhow::bail!("error: unknown switch `{}'\n{}", &arg[1..2], FMT_MERGE_MSG_USAGE);
                } else {
                    anyhow::bail!("{}", FMT_MERGE_MSG_USAGE);
                }
            }
        }
    }

    opts.shortlog_len = match shortlog_len {
        Some(n) => n,
        None => merge_log_config()?,
    };
    Ok(opts)
}

/// Reads `merge.log` (or its old name `merge.summary`) like git: a boolean (`true` means 20
/// entries) or a non-negative count.
// Used by `parse_fmt_merge_msg_args`.
fn merge_log_config() -> anyhow::Result<usize> {
    let mut len = 0;
    for key in ["merge.log", "merge.summary"] {
        let Some(value) = get_config_value(key).ok().flatten() else { continue };
        len = match value.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" => DEFAULT_MERGE_LOG_LEN,
            "false" | "no" | "off" | "" => 0,
            number => match number.parse::<i64>() {
                Ok(n) if n >= 0 => n as usize,
                Ok(_) => anyhow::bail!("error: {}: negative length {}\nfatal: unable to parse '{}' from command-line config", key, value, key),
                Err(_) => anyhow::bail!("fatal: bad boolean config value '{}' for '{}'", value, key),
            },
        };
    }
    Ok(len)
}

/// Reads a boolean config key; unset means false.
// Used by `fmt-merge-msg` (internal to this file's helpers).
fn config_bool(key: &str) -> bool {
    get_config_value(key).ok().flatten().is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"))
}

/// Port of git's `fmt_merge_msg`: turns `FETCH_HEAD`-style lines
/// (`<hash>\t\tbranch 'topic' of <repo>`) into a merge commit message. Lines marked
/// `not-for-merge` and tips already contained in HEAD or in another tip are left out. The
/// title names what is merged, grouped by repository, and ends with ` into <branch>` unless
/// the branch matches `merge.suppressDest` (`main` and `master` by default); merged tags
/// contribute their messages; and with a shortlog length each tip gets a `* name:` section
/// listing its new commits, preceded by `# By` / `# Via` credit lines when they aren't all
/// by the current user. Errors are fatal messages (exit 128).
// Used by `fmt-merge-msg`.
pub fn format_merge_message(input: &[u8], opts: &FmtMergeMsgOptions) -> anyhow::Result<Vec<u8>> {
    let head = resolve_ref_unsafe("HEAD", true, true).and_then(|r| r.hash.map(|hash| (r.name, hash)));
    let Some((head_ref, head_hash)) = head else { anyhow::bail!("fatal: No current branch") };
    let current_branch = match &opts.into_name {
        Some(name) => name.clone(),
        None => head_ref.strip_prefix("refs/heads/").unwrap_or(&head_ref).to_string(),
    };

    let text = String::from_utf8_lossy(input).into_owned();
    let lines: Vec<&str> = if text.is_empty() { Vec::new() } else { text.strip_suffix('\n').unwrap_or(&text).split('\n').collect() };
    let merge_parents = find_merge_parents(&lines, &head_hash)?;

    let mut sources: Vec<MergeSource> = Vec::new();
    let mut origins: Vec<MergeOrigin> = Vec::new();
    for (number, line) in lines.iter().enumerate() {
        if !handle_fetch_head_line(line, &merge_parents, &mut sources, &mut origins) {
            anyhow::bail!("fatal: error in line {}: {}", number + 1, line);
        }
    }

    let mut out: Vec<u8> = opts.message.clone().unwrap_or_default().into_bytes();
    if opts.message.is_none() && !sources.is_empty() {
        out.extend_from_slice(merge_title(&sources, &current_branch).as_bytes());
    }
    if !origins.is_empty() {
        append_tag_messages(&mut out, &origins);
    }
    if opts.shortlog_len > 0 {
        complete_line(&mut out);
        for origin in &origins {
            append_origin_shortlog(&mut out, origin, &head_hash, opts.shortlog_len)?;
        }
    }
    complete_line(&mut out);
    Ok(out)
}

/// git's `strbuf_complete_line`: ends a non-empty buffer with a newline.
// Used by `fmt-merge-msg` (internal to this file's helpers).
fn complete_line(out: &mut Vec<u8>) {
    if out.last().is_some_and(|&b| b != b'\n') {
        out.push(b'\n');
    }
}

/// Port of git's `find_merge_parents`: collects the `(object named on the line, commit it
/// peels to)` pairs of all for-merge lines, then drops those whose commit is HEAD itself or
/// an ancestor of HEAD or of another tip (`reduce_heads`), since merging them adds nothing.
// Used by `format_merge_message`.
fn find_merge_parents(lines: &[&str], head: &str) -> anyhow::Result<Vec<(String, String)>> {
    let mut table: Vec<(String, String)> = Vec::new();
    let mut tips: Vec<String> = Vec::new();
    for line in lines {
        let bytes = line.as_bytes();
        if bytes.len() < 42 || !bytes[..40].iter().all(|b| b.is_ascii_hexdigit()) || &bytes[40..42] != b"\t\t" {
            continue;
        }
        let given = line[..40].to_ascii_lowercase();
        let Ok(commit) = peel_to_type(given.clone(), "commit") else { continue };
        if !tips.contains(&commit) {
            tips.push(commit.clone());
        }
        if !table.iter().any(|(g, c)| *g == given && *c == commit) {
            table.push((given, commit));
        }
    }
    if !tips.iter().any(|tip| tip == head) {
        tips.push(head.to_string());
    }

    let mut independent = Vec::new();
    for tip in &tips {
        let mut redundant = false;
        for other in tips.iter().filter(|other| *other != tip) {
            if is_reachable(other, tip)? {
                redundant = true;
                break;
            }
        }
        if !redundant {
            independent.push(tip.clone());
        }
    }
    table.retain(|(_, commit)| independent.contains(commit));
    Ok(table)
}

/// Port of git's `handle_line`: files one `FETCH_HEAD` line under its repository (branch,
/// remote-tracking branch, tag, or other commit) and records its origin for the tag
/// messages and shortlog. Returns false for a malformed line.
// Used by `format_merge_message`.
fn handle_fetch_head_line(line: &str, merge_parents: &[(String, String)], sources: &mut Vec<MergeSource>, origins: &mut Vec<MergeOrigin>) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() < 43 || bytes[40] != b'\t' {
        return false;
    }
    if bytes[41..].starts_with(b"not-for-merge") {
        return true;
    }
    if bytes[41] != b'\t' || !bytes[..40].iter().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let hash = line[..40].to_ascii_lowercase();
    if !merge_parents.iter().any(|(given, _)| *given == hash) {
        return true;
    }

    // What is left reads like "branch 'frotz' of git://that/repository.git".
    let rest = &line[42..];
    let (description, src, pulling_head) = match rest.split_once(" of ") {
        Some((description, src)) => (description, src, false),
        None => (rest, rest, true),
    };

    let index = match sources.iter().position(|s| s.name == src) {
        Some(index) => index,
        None => {
            sources.push(MergeSource { name: src.to_string(), ..Default::default() });
            sources.len() - 1
        }
    };
    let source = &mut sources[index];

    let mut is_local_branch = false;
    let origin: &str = if pulling_head {
        source.head_status |= 1;
        src
    } else {
        source.head_status |= 2;
        if let Some(name) = description.strip_prefix("branch ") {
            is_local_branch = true;
            source.branches.push(name.to_string());
            name
        } else if let Some(name) = description.strip_prefix("tag ") {
            source.tags.push(name.to_string());
            description
        } else if let Some(name) = description.strip_prefix("remote-tracking branch ") {
            source.remote_branches.push(name.to_string());
            name
        } else {
            source.generic.push(description.to_string());
            src
        }
    };

    let name = if src == "." || src == origin {
        if origin.len() >= 2 && origin.starts_with('\'') && origin.ends_with('\'') { origin[1..origin.len() - 1].to_string() } else { origin.to_string() }
    } else {
        format!("{} of {}", origin, src)
    };
    origins.push(MergeOrigin { name, hash, is_local_branch: is_local_branch && src == "." });
    true
}

/// git's `print_joined`: `branch 'a'`, or `branches 'a', 'b' and 'c'`.
// Used by `merge_title`.
fn print_joined(out: &mut String, singular: &str, plural: &str, list: &[String]) {
    match list {
        [] => {}
        [only] => out.push_str(&format!("{}{}", singular, only)),
        [first @ .., last] => out.push_str(&format!("{}{} and {}", plural, first.join(", "), last)),
    }
}

/// Port of git's `fmt_merge_msg_title`: `Merge branch 'x' of <repo>; tag 'y' of <other>
/// into <branch>`, where ` into <branch>` is left out for branches matching
/// `merge.suppressDest` (by default `main` and `master`).
// Used by `format_merge_message`.
fn merge_title(sources: &[MergeSource], current_branch: &str) -> String {
    let mut out = String::from("Merge ");
    for (i, source) in sources.iter().enumerate() {
        if i > 0 {
            out.push_str("; ");
        }
        if source.head_status == 1 {
            out.push_str(&source.name);
            continue;
        }
        let mut subsep = "";
        if source.head_status == 3 {
            subsep = ", ";
            out.push_str("HEAD");
        }
        for (singular, plural, list) in [
            ("branch ", "branches ", &source.branches),
            ("remote-tracking branch ", "remote-tracking branches ", &source.remote_branches),
            ("tag ", "tags ", &source.tags),
            ("commit ", "commits ", &source.generic),
        ] {
            if !list.is_empty() {
                out.push_str(subsep);
                subsep = ", ";
                print_joined(&mut out, singular, plural, list);
            }
        }
        if source.name != "." {
            out.push_str(&format!(" of {}", source.name));
        }
    }

    out.push_str(&merge_into_suffix(current_branch));
    out.push('\n');
    out
}

/// Reads `branch.<name>.description` like git's `read_branch_desc`; `None` when the branch
/// has none. The `\n`, `\t`, `\"` and `\\` escapes git writes for multi-line descriptions
/// are decoded here, since rgit's config reader only decodes them inside quotes.
// Used by `fmt-merge-msg` (`merge.branchdesc`) and `request-pull`.
pub fn read_branch_description(branch: &str) -> Option<String> {
    let raw = get_config_value(&format!("branch.{}.description", branch)).ok().flatten().filter(|d| !d.is_empty())?;
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    Some(out)
}

/// git's `strbuf_add_commented_lines`: every line prefixed with `# ` (just `#` when empty).
// Used by `append_tag_messages`.
fn commented_lines(text: &str) -> String {
    let mut out = String::new();
    for line in text.strip_suffix('\n').unwrap_or(text).split('\n') {
        out.push_str(if line.is_empty() { "#" } else { "# " });
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Port of git's `fmt_merge_msg_sigs`: appends the message of every merged annotated tag
/// (without any signature block, which rgit cannot verify); with more than one tag each
/// message is introduced by a `# tag 'name' ...` comment line.
// Used by `format_merge_message`.
fn append_tag_messages(out: &mut Vec<u8>, origins: &[MergeOrigin]) {
    let mut tagbuf = String::new();
    let mut tag_number = 0;
    let mut first_tag = 0;
    for (i, origin) in origins.iter().enumerate() {
        let Ok((kind, content)) = read_object(&origin.hash) else { continue };
        if kind != "tag" {
            continue;
        }
        let text = String::from_utf8_lossy(&content).into_owned();
        let signature_start = ["-----BEGIN PGP SIGNATURE-----", "-----BEGIN PGP MESSAGE-----", "-----BEGIN SSH SIGNATURE-----", "-----BEGIN SIGNED MESSAGE-----"]
            .iter()
            .filter_map(|marker| if text.starts_with(marker) { Some(0) } else { text.find(&format!("\n{}", marker)).map(|p| p + 1) })
            .min();
        let payload = &text[..signature_start.unwrap_or(text.len())];
        let body = payload.split_once("\n\n").map(|(_, body)| body).unwrap_or("");

        tag_number += 1;
        if tag_number == 1 {
            first_tag = i;
        } else {
            if tag_number == 2 {
                tagbuf.insert_str(0, &format!("\n{}", commented_lines(&origins[first_tag].name)));
            }
            tagbuf.push('\n');
            tagbuf.push_str(&commented_lines(&origin.name));
        }
        tagbuf.push_str(body);
        if !tagbuf.is_empty() && !tagbuf.ends_with('\n') {
            tagbuf.push('\n');
        }
    }
    if !tagbuf.is_empty() {
        out.push(b'\n');
        out.extend_from_slice(tagbuf.as_bytes());
    }
}

/// git's `add_people_count`: `A`, `A (2) and B (1)`, or `A (5) and others`.
// Used by `append_origin_shortlog`.
fn people_count(people: &[(String, usize)]) -> String {
    match people {
        [] => String::new(),
        [(name, _)] => name.clone(),
        [(a, na), (b, nb)] => format!("{} ({}) and {} ({})", a, na, b, nb),
        [(a, na), ..] => format!("{} ({}) and others", a, na),
    }
}

/// Port of git's `shortlog` in fmt-merge-msg.c: appends the `* <name>:` section of one
/// merged tip, listing the subjects of its commits that HEAD doesn't have yet (merges are
/// not listed), at most `limit` of them followed by `...`, with the total when there are
/// more. Before it come `# By <authors>` and `# Via <committers>` comment lines unless the
/// only person involved is the current user.
// Used by `format_merge_message`.
fn append_origin_shortlog(out: &mut Vec<u8>, origin: &MergeOrigin, head: &str, limit: usize) -> anyhow::Result<()> {
    let Ok(branch) = peel_to_type(origin.hash.clone(), "") else { return Ok(()) };
    if !read_object(&branch).is_ok_and(|(kind, _)| kind == "commit") {
        return Ok(());
    }

    let mut roots = RevListRoots::default();
    roots.pending.push((branch, false, false));
    roots.pending.push((head.to_string(), true, false));
    roots.bottoms.insert(head.to_string());
    let (walked, state) = run_rev_list_walk(&RevListOptions::default(), &roots, &|_, _| true)?;

    let mut subjects: Vec<String> = Vec::new();
    let (mut authors, mut committers): (BTreeMap<String, usize>, BTreeMap<String, usize>) = (BTreeMap::new(), BTreeMap::new());
    let record = |people: &mut BTreeMap<String, usize>, name: &str| {
        let name = name.trim_end();
        if !name.is_empty() {
            *people.entry(name.to_string()).or_insert(0) += 1;
        }
    };
    let mut count = 0;
    for w in walked.iter().filter(|w| !w.hidden && !state.uninteresting.contains(&w.commit.hash)) {
        let commit = &w.commit;
        if commit.parents.len() > 1 {
            record(&mut committers, &commit.committer.name);
            continue;
        }
        if count == 0 {
            record(&mut committers, &commit.committer.name);
        }
        record(&mut authors, &commit.author.name);
        count += 1;
        if subjects.len() > limit {
            continue;
        }
        let subject = split_commit_message(&commit.message).0;
        subjects.push(if subject.is_empty() { commit.hash.clone() } else { subject });
    }

    let me = author_identity().unwrap_or_default();
    for (label, people) in [("By", authors), ("Via", committers)] {
        let mut people: Vec<(String, usize)> = people.into_iter().collect();
        people.sort_by(|a, b| b.1.cmp(&a.1));
        let only_me = people.len() == 1 && me.strip_prefix(people[0].0.as_str()).is_some_and(|rest| rest.starts_with(" <"));
        if people.is_empty() || only_me {
            continue;
        }
        out.extend_from_slice(format!("\n# {} {}", label, people_count(&people)).as_bytes());
    }

    if count > limit {
        out.extend_from_slice(format!("\n* {}: ({} commits)\n", origin.name, count).as_bytes());
    } else {
        out.extend_from_slice(format!("\n* {}:\n", origin.name).as_bytes());
    }

    if origin.is_local_branch && config_bool("merge.branchdesc") {
        if let Some(description) = read_branch_description(&origin.name) {
            for line in description.split_inclusive('\n') {
                out.extend_from_slice(format!("  : {}", line).as_bytes());
            }
            complete_line(out);
        }
    }

    for (i, subject) in subjects.iter().enumerate() {
        if i >= limit {
            out.extend_from_slice(b"  ...\n");
        } else {
            out.extend_from_slice(format!("  {}\n", subject).as_bytes());
        }
    }
    Ok(())
}

/// ` into <branch>` for a merge title, or nothing when the branch matches
/// `merge.suppressDest` (by default `main` and `master`; an empty value clears the list).
// Used by `fmt-merge-msg` and `merge`.
pub fn merge_into_suffix(current_branch: &str) -> String {
    let configured: Vec<String> = read_all_config_entries().into_iter().filter(|(key, _)| key == "merge.suppressdest").map(|(_, value)| value).collect();
    let mut patterns: Vec<String> = Vec::new();
    for value in &configured {
        if value.is_empty() {
            patterns.clear();
        } else {
            patterns.push(value.clone());
        }
    }
    if configured.is_empty() {
        patterns = vec!["main".to_string(), "master".to_string()];
    }
    if patterns.iter().any(|pattern| wildmatch(pattern, current_branch, WM_PATHNAME)) {
        String::new()
    } else {
        format!(" into {}", current_branch)
    }
}

