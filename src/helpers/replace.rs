use std::collections::BTreeMap;
use std::fs;
use crate::core::object_store::{read_object, write_object};
use crate::helpers::rebase::run_editor;
use crate::helpers::rev_list::list_all_refs;
use crate::core::rev_parse::{peel_to_type, resolve_rev};
use crate::core::wildmatch::wildmatch;
use crate::objects::{ReplaceMode, ReplaceOptions};
use crate::core::refs::{remove_packed_ref, write_ref};

/// git's usage text for `replace`, printed for `-h` and after usage errors (exit 129).
pub const REPLACE_USAGE: &str = "usage: rgit replace [-f] <object> <replacement>
   or: rgit replace [-f] --edit <object>
   or: rgit replace [-f] --graft <commit> [<parent>...]
   or: rgit replace -d <object>...
   or: rgit replace [--format=<format>] [-l [<pattern>]]

    -l, --list            list replace refs
    -d, --delete          delete replace refs
    -e, --edit            edit existing object
    -g, --graft           change a commit's parents
    -f, --force           replace the ref if it exists
    --raw                 do not pretty-print contents for --edit
    --format <format>     use this format
";

/// Parses `replace` arguments like git: one of the modes `-l`, `-d`, `-e`, `-g` (or none:
/// list without arguments, replace with them), plus `-f`, `--raw` and `--format`, and the
/// same sanity checks on how they combine and how many arguments each mode takes. Errors
/// carry the usage text and mean exit 129.
// Used by `replace`.
pub fn parse_replace_args(args: &[String]) -> anyhow::Result<ReplaceOptions> {
    let mut opts = ReplaceOptions::default();
    let mut mode: Option<ReplaceMode> = None;
    let usage = |message: &str| anyhow::anyhow!("fatal: {}\n\n{}", message, REPLACE_USAGE);
    let set_mode = |new: ReplaceMode, mode: &mut Option<ReplaceMode>| -> anyhow::Result<()> {
        if mode.is_some_and(|old| old != new) {
            anyhow::bail!("error: options cannot be used together\n{}", REPLACE_USAGE);
        }
        *mode = Some(new);
        Ok(())
    };

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        match arg {
            "--" => {
                opts.args.extend(args[i..].iter().cloned());
                break;
            }
            "-l" | "--list" => set_mode(ReplaceMode::List, &mut mode)?,
            "-d" | "--delete" => set_mode(ReplaceMode::Delete, &mut mode)?,
            "-e" | "--edit" => set_mode(ReplaceMode::Edit, &mut mode)?,
            "-g" | "--graft" => set_mode(ReplaceMode::Graft, &mut mode)?,
            "-f" | "--force" => opts.force = true,
            "--no-force" => opts.force = false,
            "--raw" => opts.raw = true,
            "--format" => {
                opts.format = Some(args.get(i).cloned().ok_or_else(|| anyhow::anyhow!("error: option `format' requires a value\n"))?);
                i += 1;
            }
            _ if arg.starts_with("--format=") => opts.format = Some(arg["--format=".len()..].to_string()),
            _ if arg.starts_with("--") => anyhow::bail!("error: unknown option `{}'\n{}", &arg[2..], REPLACE_USAGE),
            _ if arg.starts_with('-') && arg.len() > 1 => anyhow::bail!("error: unknown switch `{}'\n{}", &arg[1..2], REPLACE_USAGE),
            _ => opts.args.push(arg.to_string()),
        }
    }

    opts.mode = mode.unwrap_or(if opts.args.is_empty() { ReplaceMode::List } else { ReplaceMode::Replace });
    if opts.format.is_some() && opts.mode != ReplaceMode::List {
        return Err(usage("--format cannot be used when not listing"));
    }
    if opts.force && matches!(opts.mode, ReplaceMode::List | ReplaceMode::Delete) {
        return Err(usage("-f only makes sense when writing a replacement"));
    }
    if opts.raw && opts.mode != ReplaceMode::Edit {
        return Err(usage("--raw only makes sense with --edit"));
    }
    let count = opts.args.len();
    match opts.mode {
        ReplaceMode::Delete if count < 1 => Err(usage("-d needs at least one argument")),
        ReplaceMode::Replace if count != 2 => Err(usage("bad number of arguments")),
        ReplaceMode::Edit if count != 1 => Err(usage("-e needs exactly one argument")),
        ReplaceMode::Graft if count < 1 => Err(usage("-g needs at least one argument")),
        ReplaceMode::List if count > 1 => Err(usage("only one pattern can be given with -l")),
        _ => Ok(opts),
    }
}

/// Returns the replace refs as `original object -> replacement object`, in name order.
// Used by `replace -l`.
pub fn list_replace_refs() -> BTreeMap<String, String> {
    list_all_refs().into_iter().filter_map(|(name, hash)| Some((name.strip_prefix("refs/replace/")?.to_string(), hash))).collect()
}

/// Port of git's `list_replace_refs`: one line per replace ref whose object name matches
/// the glob `pattern` (all when `None`). `short` prints the replaced object, `medium`
/// `<object> -> <replacement>`, and `long` adds each one's type in parentheses.
// Used by `replace -l`.
pub fn format_replace_list(pattern: Option<&str>, format: Option<&str>) -> anyhow::Result<String> {
    let format = format.filter(|f| !f.is_empty()).unwrap_or("short");
    if !matches!(format, "short" | "medium" | "long") {
        anyhow::bail!("error: invalid replace format '{}'\nvalid formats are 'short', 'medium' and 'long'", format);
    }
    let kind = |hash: &str| read_object(hash).map(|(kind, _)| kind).unwrap_or_else(|_| "unknown".to_string());
    let mut out = String::new();
    for (original, replacement) in list_replace_refs() {
        if pattern.is_some_and(|p| !wildmatch(p, &original, 0)) {
            continue;
        }
        match format {
            "short" => out.push_str(&format!("{}\n", original)),
            "medium" => out.push_str(&format!("{} -> {}\n", original, replacement)),
            _ => out.push_str(&format!("{} ({}) -> {} ({})\n", original, kind(&original), replacement, kind(&replacement))),
        }
    }
    Ok(out)
}

/// Port of git's `delete_replace_ref` loop: removes `refs/replace/<object>` (loose and
/// packed) for each name, returning the lines to print and whether any name had no replace ref.
// Used by `replace -d`.
pub fn delete_replace_refs(names: &[String]) -> (Vec<String>, bool) {
    let existing = list_replace_refs();
    let (mut messages, mut had_error) = (Vec::new(), false);
    for name in names {
        let Ok(hash) = resolve_rev(name) else {
            messages.push(format!("error: failed to resolve '{}' as a valid ref", name));
            had_error = true;
            continue;
        };
        if !existing.contains_key(&hash) {
            messages.push(format!("error: replace ref '{}' not found", hash));
            had_error = true;
            continue;
        }
        let ref_name = format!("refs/replace/{}", hash);
        let _ = fs::remove_file(crate::core::repo::path(format!(".git/{}", ref_name)));
        let _ = remove_packed_ref(&ref_name);
        messages.push(format!("Deleted replace ref '{}'", hash));
    }
    (messages, had_error)
}

/// Port of git's `replace_object_oid`: points `refs/replace/<object>` at `replacement`.
/// Unless forced, the ref must not exist yet and both objects must have the same type; an
/// object can't replace itself.
// Used by `replace <object> <replacement>`, `--graft` and `--edit`.
pub fn create_replace_ref(object_name: &str, object: &str, replacement_name: &str, replacement: &str, force: bool) -> anyhow::Result<()> {
    let kind = |hash: &str| read_object(hash).map(|(kind, _)| kind).unwrap_or_default();
    let (object_type, replacement_type) = (kind(object), kind(replacement));
    if !force && object_type != replacement_type {
        anyhow::bail!(
            "error: Objects must be of the same type.\n'{}' points to a replaced object of type '{}'\nwhile '{}' points to a replacement object of type '{}'.",
            object_name,
            object_type,
            replacement_name,
            replacement_type
        );
    }
    let ref_name = format!("refs/replace/{}", object);
    if !force && list_replace_refs().contains_key(object) {
        anyhow::bail!("error: replace ref '{}' already exists", ref_name);
    }
    if object == replacement {
        anyhow::bail!("error: new object is the same as the old one: '{}'", object);
    }
    write_ref(&ref_name, replacement)
}

/// Port of git's `create_graft`: writes a copy of `commit` whose parents are `parents`
/// (each peeled to a commit; none makes it a root commit), dropping any signature, and
/// returns the new commit. The rest of the commit is kept byte for byte.
// Used by `replace --graft`.
pub fn write_grafted_commit(commit: &str, parents: &[String]) -> anyhow::Result<String> {
    let (kind, content) = read_object(commit)?;
    if kind != "commit" {
        anyhow::bail!("error: could not parse {} as a commit", commit);
    }
    let mut new_parents = Vec::new();
    for parent in parents {
        let hash = resolve_rev(parent).map_err(|_| anyhow::anyhow!("error: not a valid object name: '{}'", parent))?;
        new_parents.push(peel_to_type(hash, "commit").map_err(|_| anyhow::anyhow!("error: could not parse {} as a commit", parent))?);
    }

    let header_end = content.windows(2).position(|w| w == b"\n\n").map(|p| p + 1).unwrap_or(content.len());
    let mut out: Vec<u8> = Vec::new();
    let mut parents_written = false;
    let mut in_signature = false;
    for line in content[..header_end].split_inclusive(|&b| b == b'\n') {
        if in_signature && line.starts_with(b" ") {
            continue;
        }
        in_signature = line.starts_with(b"gpgsig ") || line.starts_with(b"gpgsig-sha256 ");
        if in_signature {
            eprintln!("warning: the original commit '{}' has a gpg signature", commit);
            eprintln!("warning: the signature will be removed in the replacement commit!");
            continue;
        }
        if line.starts_with(b"parent ") {
            continue;
        }
        if !parents_written && !line.starts_with(b"tree ") {
            for parent in &new_parents {
                out.extend_from_slice(format!("parent {}\n", parent).as_bytes());
            }
            parents_written = true;
        }
        out.extend_from_slice(line);
    }
    out.extend_from_slice(&content[header_end..]);
    write_object("commit", &out)
}

/// Port of git's `edit_and_replace` body: writes the object to a temporary file (a tree as
/// `ls-tree` lines unless `raw`), runs the editor on it, and stores the result as a new
/// object of the same type, returning its id.
// Used by `replace --edit`.
pub fn edit_object_copy(object: &str, raw: bool) -> anyhow::Result<String> {
    let (kind, content) = read_object(object)?;
    let pretty_tree = kind == "tree" && !raw;
    let text = if pretty_tree { tree_as_listing(&content) } else { content.clone() };

    let path = crate::core::repo::path(".git/REPLACE_EDITOBJ");
    fs::write(&path, &text)?;
    let edited = run_editor(&path, false).map_err(|_| anyhow::anyhow!("error: editing object file failed")).and_then(|_| Ok(fs::read(&path)?));
    let _ = fs::remove_file(&path);
    let edited = edited?;

    let new_content = if pretty_tree { listing_as_tree(&edited)? } else { edited };
    write_object(&kind, &new_content)
}

/// Renders raw tree content as `ls-tree` lines: `<mode> <type> <hash>\t<name>`.
// Used by `edit_object_copy`.
fn tree_as_listing(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < content.len() {
        let Some(space) = content[pos..].iter().position(|&b| b == b' ').map(|p| p + pos) else { break };
        let Some(nul) = content[space..].iter().position(|&b| b == 0).map(|p| p + space) else { break };
        if nul + 21 > content.len() {
            break;
        }
        let mode = String::from_utf8_lossy(&content[pos..space]).into_owned();
        let kind = match mode.as_str() {
            "40000" => "tree",
            "160000" => "commit",
            _ => "blob",
        };
        out.extend_from_slice(format!("{:0>6} {} {}\t", mode, kind, hex::encode(&content[nul + 1..nul + 21])).as_bytes());
        out.extend_from_slice(&content[space + 1..nul]);
        out.push(b'\n');
        pos = nul + 21;
    }
    out
}

/// Parses `ls-tree` lines back into raw tree content, in git's tree entry order.
// Used by `edit_object_copy`.
fn listing_as_tree(listing: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut entries: Vec<(Vec<u8>, String, Vec<u8>)> = Vec::new();
    for line in listing.split(|&b| b == b'\n').filter(|line| !line.is_empty()) {
        let bad = || anyhow::anyhow!("error: unable to parse tree entry: {}", String::from_utf8_lossy(line));
        let tab = line.iter().position(|&b| b == b'\t').ok_or_else(bad)?;
        let head = String::from_utf8_lossy(&line[..tab]).into_owned();
        let mut fields = head.split_whitespace();
        let (mode, _kind, hash) = (fields.next().ok_or_else(bad)?, fields.next().ok_or_else(bad)?, fields.next().ok_or_else(bad)?);
        let hash = hex::decode(hash).ok().filter(|h| h.len() == 20).ok_or_else(bad)?;
        entries.push((line[tab + 1..].to_vec(), mode.trim_start_matches('0').to_string(), hash));
    }
    let sort_key = |name: &[u8], mode: &str| {
        let mut key = name.to_vec();
        if mode == "40000" {
            key.push(b'/');
        }
        key
    };
    entries.sort_by(|a, b| sort_key(&a.0, &a.1).cmp(&sort_key(&b.0, &b.1)));
    let mut out = Vec::new();
    for (name, mode, hash) in entries {
        out.extend_from_slice(mode.as_bytes());
        out.push(b' ');
        out.extend_from_slice(&name);
        out.push(0);
        out.extend_from_slice(&hash);
    }
    Ok(out)
}
