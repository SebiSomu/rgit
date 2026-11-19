use std::collections::{BTreeMap, HashSet};

use anyhow::{anyhow, bail, Result};

use crate::helpers::mktree::write_tree_from_entries;
use crate::core::object_store::{flatten_tree, read_object, write_object};
use crate::core::rev_parse::{peel_to_type, resolve_rev};
use crate::helpers::update_ref::unquote_c_style;
use crate::objects::{FastImportInput, FastImportState};

/// The next line of the stream without its newline, or `None` at the end of the input.
// Used by `run_fast_import`, `import_commit`, `line_with` and `read_data`.
fn next_line<'a>(input: &mut FastImportInput<'a>) -> Option<&'a [u8]> {
    if input.pos >= input.data.len() {
        return None;
    }
    let end = input.data[input.pos..].iter().position(|b| *b == b'\n').map(|i| input.pos + i).unwrap_or(input.data.len());
    let line = &input.data[input.pos..end];
    input.pos = (end + 1).min(input.data.len());
    Some(line)
}

/// The next line if it starts with `prefix` (returning the rest); otherwise nothing is consumed.
// Used by `run_fast_import` and `import_commit` for optional lines such as `mark` and `from`.
fn line_with(input: &mut FastImportInput, prefix: &str) -> Option<String> {
    let start = input.pos;
    match next_line(input) {
        Some(line) if line.starts_with(prefix.as_bytes()) => Some(String::from_utf8_lossy(&line[prefix.len()..]).to_string()),
        _ => {
            input.pos = start;
            None
        }
    }
}

/// Reads a `data` command: `data <count>` followed by that many raw bytes and an optional
/// newline, or `data <<DELIM` followed by lines up to a line that is exactly `DELIM`.
// Used by `run_fast_import` and `import_commit`.
fn read_data(input: &mut FastImportInput) -> Result<Vec<u8>> {
    let line = next_line(input).ok_or_else(|| anyhow!("fatal: EOF in data (expected a data command)"))?;
    let spec = String::from_utf8_lossy(line).to_string();
    let Some(spec) = spec.strip_prefix("data ") else { bail!("fatal: Expected 'data n' command, found: {}", spec) };
    if let Some(delimiter) = spec.strip_prefix("<<") {
        let mut out = Vec::new();
        loop {
            let line = next_line(input).ok_or_else(|| anyhow!("fatal: EOF in data (terminator '{}' not found)", delimiter))?;
            if line == delimiter.as_bytes() {
                return Ok(out);
            }
            out.extend_from_slice(line);
            out.push(b'\n');
        }
    }
    let count: usize = spec.trim().parse().map_err(|_| anyhow!("fatal: Expected 'data n' command, found: data {}", spec))?;
    let bytes = input.data.get(input.pos..input.pos + count).ok_or_else(|| anyhow!("fatal: EOF in data ({} bytes remaining)", count))?;
    input.pos += count;
    if input.data.get(input.pos) == Some(&b'\n') {
        input.pos += 1;
    }
    Ok(bytes.to_vec())
}

/// Parses `:<n>` into the mark number.
// Used by `run_fast_import`.
fn mark_number(text: &str) -> Result<u64> {
    text.strip_prefix(':').and_then(|n| n.trim().parse().ok()).ok_or_else(|| anyhow!("fatal: Expected mark, found: {}", text))
}

/// Resolves an object reference of the stream: a mark (`:5`), a full object name, a ref the
/// stream is building, or any revision expression of the repository.
// Used by `run_fast_import`.
fn resolve_reference(state: &FastImportState, text: &str) -> Result<String> {
    let text = text.trim();
    if text.starts_with(':') {
        let mark = mark_number(text)?;
        return state.marks.get(&mark).cloned().ok_or_else(|| anyhow!("fatal: mark :{} not declared", mark));
    }
    if text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(text.to_ascii_lowercase());
    }
    if let Some(Some(tip)) = state.refs.get(text) {
        return Ok(tip.clone());
    }
    // `<ref>^0` and similar on a ref built in this stream.
    if let Some((name, suffix)) = text.split_once(['^', '~']).map(|(name, _)| (name, &text[name.len()..])) {
        if let Some(Some(tip)) = state.refs.get(name) {
            return resolve_rev(&format!("{}{}", tip, suffix)).map_err(|_| anyhow!("fatal: Not a valid commit: {}", text));
        }
    }
    resolve_rev(text).map_err(|_| anyhow!("fatal: Not a valid commit: {}", text))
}

/// Reads a path argument: C-style quoted when it starts with a double quote, else the text
/// up to the end (`to_end`) or up to the next space. Returns the path and the remaining text.
// Used by `run_fast_import` for `M`, `D`, `C` and `R`.
fn take_path(text: &str, to_end: bool) -> Result<(String, String)> {
    if text.starts_with('"') {
        let (bytes, end) = unquote_c_style(text.as_bytes(), 0).ok_or_else(|| anyhow!("fatal: Invalid path: {}", text))?;
        return Ok((String::from_utf8_lossy(&bytes).to_string(), text[end..].trim_start_matches(' ').to_string()));
    }
    if to_end {
        return Ok((text.to_string(), String::new()));
    }
    match text.split_once(' ') {
        Some((path, rest)) => Ok((path.to_string(), rest.to_string())),
        None => Ok((text.to_string(), String::new())),
    }
}

/// Removes a path from a flattened tree: the file itself and everything below it.
// Used by `run_fast_import`.
fn remove_path(tree: &mut BTreeMap<String, ([u8; 20], u32)>, path: &str) {
    if path.is_empty() {
        tree.clear();
        return;
    }
    let below = format!("{}/", path);
    tree.retain(|name, _| name != path && !name.starts_with(&below));
}

/// Puts an entry at a path of a flattened tree, replacing a directory there and any file
/// that stands where one of the path's directories has to be. A tree object is expanded.
// Used by `run_fast_import`.
fn set_path(tree: &mut BTreeMap<String, ([u8; 20], u32)>, path: &str, hash: [u8; 20], mode: u32) -> Result<()> {
    remove_path(tree, path);
    let mut prefix = String::new();
    for part in path.split('/') {
        if !prefix.is_empty() {
            tree.remove(&prefix);
            prefix.push('/');
        }
        prefix.push_str(part);
    }
    if mode == 0o040000 {
        let base = if path.is_empty() { String::new() } else { format!("{}/", path) };
        flatten_tree(&hex::encode(hash), &base, tree)?;
    } else {
        tree.insert(path.to_string(), (hash, mode));
    }
    Ok(())
}

/// Writes the tree objects for a flattened tree and returns the root tree's hash.
// Used by `run_fast_import`.
pub fn write_flat_tree(files: &BTreeMap<String, ([u8; 20], u32)>, created: &mut usize) -> Result<String> {
    let mut entries: Vec<(u32, [u8; 20], Vec<u8>)> = Vec::new();
    let mut subdirs: BTreeMap<String, BTreeMap<String, ([u8; 20], u32)>> = BTreeMap::new();
    for (path, (hash, mode)) in files {
        match path.split_once('/') {
            Some((dir, rest)) => {
                subdirs.entry(dir.to_string()).or_default().insert(rest.to_string(), (*hash, *mode));
            }
            None => entries.push((*mode, *hash, path.clone().into_bytes())),
        }
    }
    for (dir, children) in &subdirs {
        let mut hash = [0u8; 20];
        hex::decode_to_slice(write_flat_tree(children, created)?, &mut hash)?;
        entries.push((0o040000, hash, dir.clone().into_bytes()));
    }
    *created += 1;
    write_tree_from_entries(&mut entries)
}

/// Checks an author / committer / tagger value and returns it as it goes into the object.
/// With the `now` date format the date part is replaced by the current time.
// Used by `run_fast_import`.
fn ident(state: &FastImportState, text: &str) -> Result<String> {
    let close = text.rfind('>').ok_or_else(|| anyhow!("fatal: Missing > in ident string: {}", text))?;
    if !text[..close].contains('<') {
        bail!("fatal: Missing < in ident string: {}", text);
    }
    let date = text[close + 1..].trim();
    match state.date_format.as_str() {
        "now" => {
            if date != "now" {
                bail!("fatal: Date in ident must be 'now': {}", text);
            }
            let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            Ok(format!("{} {} +0000", &text[..=close], seconds))
        }
        _ => {
            let mut parts = date.split(' ');
            let valid_time = parts.next().is_some_and(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()));
            let valid_zone = parts.next().is_some_and(|z| z.len() >= 5 && (z.starts_with('+') || z.starts_with('-')) && z[1..].bytes().all(|b| b.is_ascii_digit()));
            if !valid_time || !valid_zone || parts.next().is_some() {
                bail!("fatal: Invalid raw date \"{}\" in ident: {}", date, text);
            }
            Ok(text.to_string())
        }
    }
}

/// Parses a file mode of an `M` command.
// Used by `run_fast_import`.
fn file_mode(text: &str) -> Result<u32> {
    match text {
        "644" | "100644" => Ok(0o100644),
        "755" | "100755" => Ok(0o100755),
        "120000" => Ok(0o120000),
        "160000" => Ok(0o160000),
        "040000" | "40000" => Ok(0o040000),
        other => bail!("fatal: Corrupt mode: {}", other),
    }
}

/// Reads a whole `fast-import` stream and creates its objects: `blob`, `commit` (with `M`,
/// `D`, `C`, `R`, `N` and `deleteall` file commands), `tag`, `reset`, `alias`, plus
/// `feature`, `option`, `progress`, `checkpoint`, `get-mark` and `done`. Refs are only
/// recorded in `state`; `finish_fast_import` writes them. Objects are stored loose.
// Used by `fast-import`.
pub fn run_fast_import(data: &[u8], state: &mut FastImportState) -> Result<()> {
    let mut input = FastImportInput { data, pos: 0 };
    let input = &mut input;
    while let Some(raw) = next_line(input) {
        let line = String::from_utf8_lossy(raw).to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "blob" {
            let mark = line_with(input, "mark ").map(|m| mark_number(&m)).transpose()?;
            line_with(input, "original-oid ");
            let hash = write_object("blob", &read_data(input)?)?;
            state.counts[0] += 1;
            if let Some(mark) = mark {
                state.marks.insert(mark, hash);
            }
        } else if let Some(refname) = line.strip_prefix("commit ") {
            import_commit(input, state, refname.trim())?;
        } else if let Some(name) = line.strip_prefix("tag ") {
            let mark = line_with(input, "mark ").map(|m| mark_number(&m)).transpose()?;
            let from = line_with(input, "from ").ok_or_else(|| anyhow!("fatal: Expected from command, got {}", String::from_utf8_lossy(next_line(input).unwrap_or_default())))?;
            let target = resolve_reference(state, &from)?;
            line_with(input, "original-oid ");
            let tagger = line_with(input, "tagger ").map(|t| ident(state, &t)).transpose()?;
            let message = read_data(input)?;
            let (kind, _) = read_object(&target).map_err(|_| anyhow!("fatal: Not a valid object: {}", from))?;
            let mut content = format!("object {}\ntype {}\ntag {}\n", target, kind, name).into_bytes();
            if let Some(tagger) = tagger {
                content.extend_from_slice(format!("tagger {}\n", tagger).as_bytes());
            }
            content.push(b'\n');
            content.extend_from_slice(&message);
            let hash = write_object("tag", &content)?;
            state.counts[3] += 1;
            if let Some(mark) = mark {
                state.marks.insert(mark, hash.clone());
            }
            state.refs.insert(format!("refs/tags/{}", name), Some(hash));
        } else if let Some(refname) = line.strip_prefix("reset ") {
            let refname = refname.trim().to_string();
            state.trees.remove(&refname);
            state.deleted.retain(|name| *name != refname);
            match line_with(input, "from ") {
                Some(from) if from.trim().bytes().all(|b| b == b'0') && from.trim().len() == 40 => {
                    state.refs.remove(&refname);
                    state.deleted.push(refname);
                }
                Some(from) => {
                    let target = resolve_reference(state, &from)?;
                    state.refs.insert(refname, Some(target));
                }
                None => {
                    state.refs.insert(refname, None);
                }
            }
        } else if line == "alias" {
            let mark = mark_number(&line_with(input, "mark ").ok_or_else(|| anyhow!("fatal: Expected mark command in alias"))?)?;
            let to = line_with(input, "to ").ok_or_else(|| anyhow!("fatal: Expected 'to' command in alias"))?;
            let target = resolve_reference(state, &to)?;
            state.marks.insert(mark, target);
        } else if let Some(message) = line.strip_prefix("progress ") {
            println!("progress {}", message);
        } else if line == "checkpoint" {
            // Everything is written as it is read; refs are stored at the end.
        } else if line == "done" {
            state.saw_done = true;
            break;
        } else if let Some(feature) = line.strip_prefix("feature ") {
            let (name, value) = feature.split_once('=').unwrap_or((feature, ""));
            match name {
                "done" => state.require_done = true,
                "force" => state.force = true,
                "date-format" => set_date_format(state, value)?,
                "export-marks" => state.export_marks = Some(value.to_string()),
                "import-marks" => import_marks(state, value, false)?,
                "import-marks-if-exists" => import_marks(state, value, true)?,
                "notes" | "relative-marks" | "no-relative-marks" | "get-mark" => {}
                other => bail!("fatal: This version of fast-import does not support feature {}.", other),
            }
        } else if let Some(option) = line.strip_prefix("option ") {
            if let Some(option) = option.strip_prefix("git ") {
                let (name, value) = option.split_once('=').unwrap_or((option, ""));
                match name {
                    "force" => state.force = true,
                    "date-format" => set_date_format(state, value)?,
                    "export-marks" => state.export_marks = Some(value.to_string()),
                    "import-marks" => import_marks(state, value, false)?,
                    "import-marks-if-exists" => import_marks(state, value, true)?,
                    "done" => state.require_done = true,
                    _ => {}
                }
            }
        } else if let Some(mark) = line.strip_prefix("get-mark ") {
            let number = mark_number(mark)?;
            println!("{}", state.marks.get(&number).ok_or_else(|| anyhow!("fatal: Unknown mark: {}", mark))?);
        } else {
            bail!("fatal: Unsupported command: {}", line);
        }
    }
    if state.require_done && !state.saw_done {
        bail!("fatal: stream ends early");
    }
    Ok(())
}

/// Sets the date format (`raw`, `raw-permissive` or `now`).
// Used by `run_fast_import` and `fast-import --date-format`.
pub fn set_date_format(state: &mut FastImportState, format: &str) -> Result<()> {
    match format {
        "raw" | "raw-permissive" | "now" => {
            state.date_format = format.to_string();
            Ok(())
        }
        other => bail!("fatal: unknown --date-format argument {} (rgit supports raw, raw-permissive and now)", other),
    }
}

/// Loads a marks file (`:<n> <hash>` per line) into the state.
// Used by `run_fast_import` and `fast-import --import-marks`.
pub fn import_marks(state: &mut FastImportState, file: &str, if_exists: bool) -> Result<()> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(_) if if_exists => return Ok(()),
        Err(e) => bail!("fatal: cannot read '{}': {}", file, e),
    };
    for line in text.lines().filter(|l| !l.is_empty()) {
        let (mark, hash) = line.split_once(' ').ok_or_else(|| anyhow!("fatal: corrupt mark line: {}", line))?;
        state.marks.insert(mark_number(mark)?, hash.trim().to_string());
    }
    Ok(())
}

/// Handles one `commit` command: header lines, message, parents and file changes; builds
/// the trees and the commit object and moves the ref's tip in `state`.
// Used by `run_fast_import`.
fn import_commit(input: &mut FastImportInput, state: &mut FastImportState, refname: &str) -> Result<()> {
    let mark = line_with(input, "mark ").map(|m| mark_number(&m)).transpose()?;
    line_with(input, "original-oid ");
    let author = line_with(input, "author ").map(|a| ident(state, &a)).transpose()?;
    let committer = ident(state, &line_with(input, "committer ").ok_or_else(|| anyhow!("fatal: Expected committer but didn't get one"))?)?;
    let encoding = line_with(input, "encoding ");
    let message = read_data(input)?;

    let mut parents: Vec<String> = Vec::new();
    let mut tree: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
    match line_with(input, "from ") {
        Some(from) => {
            let parent = peel_to_type(resolve_reference(state, &from)?, "commit").map_err(|_| anyhow!("fatal: Not a valid commit: {}", from))?;
            // The ref's own tip: its tree is already in memory.
            match state.refs.get(refname) {
                Some(Some(tip)) if *tip == parent && state.trees.contains_key(refname) => tree = state.trees[refname].clone(),
                _ => flatten_tree(&peel_to_type(parent.clone(), "tree")?, "", &mut tree)?,
            }
            parents.push(parent);
        }
        None => {
            if let Some(Some(tip)) = state.refs.get(refname) {
                match state.trees.get(refname) {
                    Some(known) => tree = known.clone(),
                    None => flatten_tree(&peel_to_type(tip.clone(), "tree")?, "", &mut tree)?,
                }
                parents.push(tip.clone());
            }
        }
    }
    while let Some(merge) = line_with(input, "merge ") {
        parents.push(peel_to_type(resolve_reference(state, &merge)?, "commit").map_err(|_| anyhow!("fatal: Not a valid commit: {}", merge))?);
    }

    loop {
        let start = input.pos;
        let Some(raw) = next_line(input) else { break };
        let line = String::from_utf8_lossy(raw).to_string();
        if let Some(rest) = line.strip_prefix("M ") {
            let (mode, rest) = rest.split_once(' ').ok_or_else(|| anyhow!("fatal: Corrupt mode: {}", line))?;
            let mode = file_mode(mode)?;
            let (reference, path) = rest.split_once(' ').ok_or_else(|| anyhow!("fatal: Missing space after SHA1: {}", line))?;
            let (path, _) = take_path(path, true)?;
            let hash = if reference == "inline" {
                state.counts[0] += 1;
                write_object("blob", &read_data(input)?)?
            } else {
                resolve_reference(state, reference)?
            };
            let mut bytes = [0u8; 20];
            hex::decode_to_slice(&hash, &mut bytes)?;
            set_path(&mut tree, &path, bytes, mode)?;
        } else if let Some(rest) = line.strip_prefix("D ") {
            remove_path(&mut tree, &take_path(rest, true)?.0);
        } else if line == "deleteall" {
            tree.clear();
        } else if line.starts_with("C ") || line.starts_with("R ") {
            let (source, rest) = take_path(&line[2..], false)?;
            let (target, _) = take_path(&rest, true)?;
            let below = format!("{}/", source);
            let moved: Vec<(String, ([u8; 20], u32))> = tree
                .iter()
                .filter(|(name, _)| **name == source || name.starts_with(&below))
                .map(|(name, entry)| (format!("{}{}", target, &name[source.len()..]), *entry))
                .collect();
            if moved.is_empty() {
                bail!("fatal: Path {} not in branch", source);
            }
            if line.starts_with("R ") {
                remove_path(&mut tree, &source);
            }
            remove_path(&mut tree, &target);
            for (name, (hash, mode)) in moved {
                set_path(&mut tree, &name, hash, mode)?;
            }
        } else if let Some(rest) = line.strip_prefix("N ") {
            // A note: stored in the notes tree under the annotated object's name.
            let (reference, target) = rest.split_once(' ').ok_or_else(|| anyhow!("fatal: Missing space after SHA1: {}", line))?;
            let note = if reference == "inline" {
                state.counts[0] += 1;
                Some(write_object("blob", &read_data(input)?)?)
            } else if reference.len() == 40 && reference.bytes().all(|b| b == b'0') {
                None
            } else {
                Some(resolve_reference(state, reference)?)
            };
            let annotated = resolve_reference(state, target)?;
            let fanned = format!("{}/{}", &annotated[..2], &annotated[2..]);
            remove_path(&mut tree, &fanned);
            remove_path(&mut tree, &annotated);
            if let Some(note) = note {
                let mut bytes = [0u8; 20];
                hex::decode_to_slice(&note, &mut bytes)?;
                tree.insert(annotated, (bytes, 0o100644));
            }
        } else {
            // A blank line ends the commit; anything else is the next command.
            if !line.is_empty() {
                input.pos = start;
            }
            break;
        }
    }

    let tree_hash = write_flat_tree(&tree, &mut state.counts[1])?;
    let mut content = format!("tree {}\n", tree_hash);
    for parent in &parents {
        content.push_str(&format!("parent {}\n", parent));
    }
    content.push_str(&format!("author {}\ncommitter {}\n", author.as_deref().unwrap_or(&committer), committer));
    if let Some(encoding) = encoding {
        content.push_str(&format!("encoding {}\n", encoding));
    }
    content.push('\n');
    let mut bytes = content.into_bytes();
    bytes.extend_from_slice(&message);
    let hash = write_object("commit", &bytes)?;
    state.counts[2] += 1;
    if let Some(mark) = mark {
        state.marks.insert(mark, hash.clone());
    }
    state.deleted.retain(|name| name != refname);
    state.refs.insert(refname.to_string(), Some(hash));
    state.trees.insert(refname.to_string(), tree);
    Ok(())
}

/// Whether `ancestor` is reachable from `tip` by following parents.
// Used by `finish_fast_import`.
fn is_ancestor(ancestor: &str, tip: &str) -> bool {
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack = vec![tip.to_string()];
    while let Some(hash) = stack.pop() {
        if hash == ancestor {
            return true;
        }
        if !seen.insert(hash.clone()) {
            continue;
        }
        let Ok((kind, content)) = read_object(&hash) else { continue };
        if kind != "commit" {
            continue;
        }
        let text = String::from_utf8_lossy(&content);
        stack.extend(text.lines().take_while(|l| !l.is_empty()).filter_map(|l| l.strip_prefix("parent ")).map(|p| p.trim().to_string()));
    }
    false
}

/// Stores what the stream decided: writes every ref to its new tip (a branch that would
/// lose commits is skipped with git's warning unless `force`), deletes the refs reset to
/// the zero hash, and writes the marks file. Returns whether a ref had to be skipped.
// Used by `fast-import`.
pub fn finish_fast_import(state: &FastImportState) -> Result<bool> {
    let mut skipped = false;
    for (name, tip) in &state.refs {
        let Some(tip) = tip else { continue };
        let old = crate::core::refs::read_ref(name).ok().flatten();
        if let Some(old) = old.filter(|old| old != tip) {
            let is_branch = !name.starts_with("refs/tags/");
            if !state.force && is_branch && !is_ancestor(&old, tip) {
                eprintln!("warning: Not updating {} (new tip {} does not contain {})", name, tip, old);
                skipped = true;
                continue;
            }
        }
        crate::core::refs::write_ref(name, tip)?;
    }
    for name in &state.deleted {
        let _ = std::fs::remove_file(crate::core::repo::path(format!(".git/{}", name)));
        let _ = crate::core::refs::remove_packed_ref(name);
    }
    if let Some(file) = &state.export_marks {
        let text: String = state.marks.iter().map(|(mark, hash)| format!(":{} {}\n", mark, hash)).collect();
        std::fs::write(file, text)?;
    }
    Ok(skipped)
}
