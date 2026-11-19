use crate::core::object_store::{read_object, write_object};
use crate::references::update_ref::engine::unquote_c_style;

/// Usage text of `mktree`, printed for `-h` and usage errors.
pub const MKTREE_USAGE: &str = "usage: rgit mktree [-z] [--missing] [--batch]\n\n    -z                    input is NUL terminated\n    --[no-]missing        allow missing objects\n    --[no-]batch          allow creation of more than one tree\n\n";

/// The object type a tree entry mode stands for: `tree` for directories, `commit` for
/// submodule links, `blob` for everything else.
// Used by `parse_mktree_line`.
fn mode_type(mode: u32) -> &'static str {
    match mode & 0o170000 {
        0o040000 => "tree",
        0o160000 => "commit",
        _ => "blob",
    }
}

/// Parses one `ls-tree` line (`<mode> SP <type> SP <hash> TAB <name>`) into
/// `(mode, hash bytes, name)`, with git's checks: the type must fit the mode and the object
/// (which must exist unless `allow_missing` or the entry is a submodule link), and the name
/// may not contain a slash. Names in double quotes are unquoted unless `nul_terminated`.
/// Errors are git's `fatal:` messages.
// Used by `mktree`.
pub fn parse_mktree_line(line: &[u8], nul_terminated: bool, allow_missing: bool) -> Result<(u32, [u8; 20], Vec<u8>), String> {
    let shown = String::from_utf8_lossy(line).to_string();
    let format_error = || format!("fatal: input format error: {}", shown);

    let mode_end = line.iter().position(|b| !(b'0'..=b'7').contains(b)).unwrap_or(line.len());
    if mode_end == 0 || line.get(mode_end) != Some(&b' ') {
        return Err(format_error());
    }
    let mode = u32::from_str_radix(&String::from_utf8_lossy(&line[..mode_end]), 8).map_err(|_| format_error())?;
    let rest = &line[mode_end + 1..];
    let type_end = rest.iter().position(|b| *b == b' ').ok_or_else(format_error)?;
    let type_name = String::from_utf8_lossy(&rest[..type_end]).to_string();
    let after_type = &rest[type_end + 1..];
    if after_type.len() < 41 || after_type[40] != b'\t' || !after_type[..40].iter().all(|b| b.is_ascii_hexdigit()) {
        return Err(format_error());
    }
    let hex_hash = String::from_utf8_lossy(&after_type[..40]).to_lowercase();
    let mut name = after_type[41..].to_vec();
    if !nul_terminated && name.first() == Some(&b'"') {
        match unquote_c_style(&name, 0) {
            Some((unquoted, end)) if end == name.len() => name = unquoted,
            _ => return Err("fatal: invalid quoting".to_string()),
        }
    }
    let shown_name = String::from_utf8_lossy(&name).to_string();

    if !matches!(type_name.as_str(), "blob" | "tree" | "commit" | "tag") {
        return Err(format!("fatal: invalid object type \"{}\"", type_name));
    }
    let expected = mode_type(mode);
    if expected != type_name {
        return Err(format!("fatal: entry '{}' object type ({}) doesn't match mode type ({})", shown_name, type_name, expected));
    }
    match read_object(&hex_hash) {
        Ok((actual, _)) if actual != expected => {
            return Err(format!("fatal: entry '{}' object {} is a {} but specified type was ({})", shown_name, hex_hash, actual, expected));
        }
        Ok(_) => {}
        Err(_) if allow_missing || expected == "commit" => {}
        Err(_) => return Err(format!("fatal: entry '{}' object {} is unavailable", shown_name, hex_hash)),
    }
    if name.contains(&b'/') {
        return Err(format!("fatal: path {} contains slash", shown_name));
    }
    let mut hash = [0u8; 20];
    hex::decode_to_slice(&hex_hash, &mut hash).map_err(|_| format_error())?;
    Ok((mode, hash, name))
}

/// Writes a tree object from entries in any order: they are sorted the way git sorts tree
/// entries (directories compare as if their name ended in `/`) and serialized as
/// `<octal mode> SP <name> NUL <20-byte hash>`. Returns the tree's hash.
// Used by `mktree`.
pub fn write_tree_from_entries(entries: &mut [(u32, [u8; 20], Vec<u8>)]) -> anyhow::Result<String> {
    let sort_key = |entry: &(u32, [u8; 20], Vec<u8>)| {
        let mut key = entry.2.clone();
        if entry.0 & 0o170000 == 0o040000 {
            key.push(b'/');
        }
        key
    };
    entries.sort_by_key(sort_key);
    let mut content = Vec::new();
    for (mode, hash, name) in entries.iter() {
        content.extend_from_slice(format!("{:o} ", mode).as_bytes());
        content.extend_from_slice(name);
        content.push(0);
        content.extend_from_slice(hash);
    }
    write_object("tree", &content)
}
