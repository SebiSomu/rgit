use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use anyhow::Context;
use flate2::bufread::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};
use crate::helpers::apply::apply_git_delta;
use crate::helpers::bisect::collect_ancestors;
use crate::helpers::format_patch::{parse_commit_object, resolve_commit_rev, split_commit_message};
use crate::helpers::merge::find_all_merge_bases;
use crate::helpers::rev_list::list_all_refs;
use crate::helpers::rev_parse::peel_to_type;
use crate::helpers::{read_object, write_object};
use crate::objects::BundleHeader;

/// One positive revision given to `bundle create`: the ref it names (if it names one), the
/// object that ref points at (a tag object for annotated tags), and the commit it peels to.
pub struct BundleTip {
    pub refname: Option<String>,
    pub object: String,
    pub commit: String,
}

/// Parses a bundle's text header: the `# v2 git bundle` / `# v3 git bundle` signature, v3
/// `@key[=value]` capabilities, `-<oid> [<comment>]` prerequisite lines and `<oid> <refname>`
/// ref lines, up to the blank line where the packfile begins.
// Used by `bundle verify`, `bundle list-heads`, and `bundle unbundle`.
pub fn parse_bundle_header(bytes: &[u8], label: &str) -> anyhow::Result<BundleHeader> {
    let not_a_bundle = || anyhow::anyhow!("error: '{}' does not look like a v2 or v3 bundle file", label);
    let mut header = BundleHeader::default();
    let mut pos = 0;
    let next_line = |pos: &mut usize| -> Option<String> {
        let end = bytes[*pos..].iter().position(|&b| b == b'\n')? + *pos;
        let line = String::from_utf8_lossy(&bytes[*pos..end]).into_owned();
        *pos = end + 1;
        Some(line)
    };

    header.version = match next_line(&mut pos).as_deref() {
        Some("# v2 git bundle") => 2,
        Some("# v3 git bundle") => 3,
        _ => return Err(not_a_bundle()),
    };
    loop {
        let line = next_line(&mut pos).ok_or_else(not_a_bundle)?;
        if line.is_empty() {
            break;
        }
        if let Some(capability) = line.strip_prefix('@').filter(|_| header.version == 3) {
            let (key, value) = match capability.split_once('=') {
                Some((k, v)) => (k.to_string(), Some(v.to_string())),
                None => (capability.to_string(), None),
            };
            match (key.as_str(), value.as_deref()) {
                ("object-format", Some("sha1")) | ("filter", _) => {}
                _ => anyhow::bail!("error: unknown capability '{}'", capability),
            }
            header.capabilities.push((key, value));
        } else if let Some(prerequisite) = line.strip_prefix('-') {
            let (oid, comment) = prerequisite.split_once(' ').unwrap_or((prerequisite, ""));
            if !is_hex_oid(oid) {
                anyhow::bail!("error: unrecognized header: {}", line);
            }
            header.prerequisites.push((oid.to_string(), comment.to_string()));
        } else {
            let (oid, name) = line.split_once(' ').ok_or_else(|| anyhow::anyhow!("error: unrecognized header: {}", line))?;
            if !is_hex_oid(oid) {
                anyhow::bail!("error: unrecognized header: {}", line);
            }
            header.refs.push((oid.to_string(), name.to_string()));
        }
    }
    header.pack_offset = pos;
    Ok(header)
}

fn is_hex_oid(text: &str) -> bool {
    text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Returns the prerequisite commits of a bundle that this repository doesn't have.
// Used by `bundle verify` and `bundle unbundle`.
pub fn missing_bundle_prerequisites(header: &BundleHeader) -> Vec<String> {
    header
        .prerequisites
        .iter()
        .filter(|(oid, _)| !matches!(read_object(oid), Ok((kind, _)) if kind == "commit"))
        .map(|(oid, _)| oid.clone())
        .collect()
}

/// Expands a revision name to the full ref it names, using git's lookup order (`<name>`,
/// `refs/<name>`, `refs/tags/<name>`, `refs/heads/<name>`, `refs/remotes/<name>`,
/// `refs/remotes/<name>/HEAD`), with the object that ref points at. Names that aren't refs
/// (hashes, `main~2`) give `None`.
// Used by `bundle create`.
pub fn dwim_ref(name: &str, refs: &BTreeMap<String, String>) -> anyhow::Result<Option<(String, String)>> {
    if name == "HEAD" {
        return Ok(crate::refs::resolve_head_commit()?.map(|hash| ("HEAD".to_string(), hash)));
    }
    let candidates = [
        name.to_string(),
        format!("refs/{}", name),
        format!("refs/tags/{}", name),
        format!("refs/heads/{}", name),
        format!("refs/remotes/{}", name),
        format!("refs/remotes/{}/HEAD", name),
    ];
    Ok(candidates.into_iter().filter(|c| c.starts_with("refs/")).find_map(|c| refs.get(&c).map(|hash| (c.clone(), hash.clone()))))
}

/// Turns `bundle create`'s revision arguments into the positive tips and the excluded
/// commits: `X` (positive), `^X`, `A..B`, `A...B` (merge bases excluded), `--not`, and
/// `--all` / `--branches` / `--tags` / `--remotes` (every matching ref, `--all` adding `HEAD`).
// Used by `bundle create`.
pub fn resolve_bundle_revs(revs: &[String]) -> anyhow::Result<(Vec<BundleTip>, Vec<String>)> {
    let refs = list_all_refs();
    let mut tips = Vec::new();
    let mut negatives = Vec::new();
    let mut negate = false;

    let resolve = |name: &str| -> anyhow::Result<String> {
        resolve_commit_rev(name).map_err(|_| {
            anyhow::anyhow!(
                "fatal: ambiguous argument '{}': unknown revision or path not in the working tree.
Use '--' to separate paths from revisions, like this:
'rgit <command> [<revision>...] -- [<file>...]'",
                name
            )
        })
    };
    let positive = |name: &str, tips: &mut Vec<BundleTip>| -> anyhow::Result<()> {
        let commit = resolve(name)?;
        match dwim_ref(name, &refs)? {
            Some((refname, object)) => tips.push(BundleTip { refname: Some(refname), object, commit }),
            None => tips.push(BundleTip { refname: None, object: commit.clone(), commit }),
        }
        Ok(())
    };

    for rev in revs {
        let rev = rev.as_str();
        match rev {
            "--not" => negate = !negate,
            "--all" | "--branches" | "--tags" | "--remotes" => {
                let prefix = match rev {
                    "--branches" => "refs/heads/",
                    "--tags" => "refs/tags/",
                    "--remotes" => "refs/remotes/",
                    _ => "refs/",
                };
                let mut matching: Vec<(String, String)> = refs.iter().filter(|(n, _)| n.starts_with(prefix)).map(|(n, h)| (n.clone(), h.clone())).collect();
                if rev == "--all" {
                    if let Some(head) = crate::refs::resolve_head_commit()? {
                        matching.push(("HEAD".to_string(), head));
                    }
                }
                for (refname, object) in matching {
                    let Ok(commit) = peel_to_type(object.clone(), "commit") else { continue };
                    if negate {
                        negatives.push(commit);
                    } else {
                        tips.push(BundleTip { refname: Some(refname), object, commit });
                    }
                }
            }
            _ if rev.starts_with('-') => anyhow::bail!("fatal: unsupported rev-list option '{}' for bundle create", rev),
            _ => {
                let (excluded, rev) = match rev.strip_prefix('^') {
                    Some(plain) => (!negate, plain),
                    None => (negate, rev),
                };
                let or_head = |s: &str| if s.is_empty() { "HEAD".to_string() } else { s.to_string() };
                if let Some((a, b)) = rev.split_once("...") {
                    let (a, b) = (or_head(a), or_head(b));
                    negatives.extend(find_all_merge_bases(&resolve(&a)?, &resolve(&b)?)?);
                    positive(&a, &mut tips)?;
                    positive(&b, &mut tips)?;
                } else if let Some((a, b)) = rev.split_once("..") {
                    negatives.push(resolve(&or_head(a))?);
                    positive(&or_head(b), &mut tips)?;
                } else if excluded {
                    negatives.push(resolve(rev)?);
                } else {
                    positive(rev, &mut tips)?;
                }
            }
        }
    }
    Ok((tips, negatives))
}

/// Selects what a bundle carries: the commits reachable from the tips but not from the
/// excluded commits, and the prerequisites, i.e. excluded commits that are parents of
/// selected ones (the receiver must already have them), newest first. Returns
/// `(commits, prerequisites)`.
// Used by `bundle create`.
pub fn select_bundle_commits(tips: &[BundleTip], negatives: &[String]) -> anyhow::Result<(HashSet<String>, Vec<String>)> {
    let mut excluded = HashSet::new();
    for negative in negatives {
        excluded.extend(collect_ancestors(negative)?);
    }
    let mut commits = HashSet::new();
    for tip in tips {
        commits.extend(collect_ancestors(&tip.commit)?.into_iter().filter(|c| !excluded.contains(c)));
    }

    let mut prerequisites = Vec::new();
    let mut seen = HashSet::new();
    for commit in &commits {
        for parent in parse_commit_object(commit)?.parents {
            if !commits.contains(&parent) && seen.insert(parent.clone()) {
                prerequisites.push(parent);
            }
        }
    }
    let mut dated = Vec::new();
    for hash in prerequisites {
        dated.push((parse_commit_object(&hash)?.committer.timestamp, hash));
    }
    dated.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    Ok((commits, dated.into_iter().map(|(_, h)| h).collect()))
}

/// Adds a tree and everything below it (subtrees and blobs, but not submodule commits) to
/// `out`, skipping objects already in `skip` or `out`.
fn collect_tree_objects(tree: &str, skip: &HashSet<String>, out: &mut HashSet<String>) -> anyhow::Result<()> {
    if skip.contains(tree) || !out.insert(tree.to_string()) {
        return Ok(());
    }
    let (_, content) = read_object(tree)?;
    let mut pos = 0;
    while pos < content.len() {
        let space = content[pos..].iter().position(|&b| b == b' ').context("corrupt tree")? + pos;
        let nul = content[space..].iter().position(|&b| b == 0).context("corrupt tree")? + space;
        let mode = &content[pos..space];
        let hash = hex::encode(&content[nul + 1..nul + 21]);
        pos = nul + 21;
        match mode {
            b"40000" => collect_tree_objects(&hash, skip, out)?,
            b"160000" => {}
            _ => {
                if !skip.contains(&hash) {
                    out.insert(hash);
                }
            }
        }
    }
    Ok(())
}

/// Lists every object a bundle must contain: the selected commits, their trees and blobs, and
/// the tag objects the named refs point at (following tag chains), minus everything already
/// reachable from the prerequisites' trees. Commits and tags come first, as in git's packs.
// Used by `bundle create`.
pub fn enumerate_bundle_objects(commits: &HashSet<String>, prerequisites: &[String], tips: &[BundleTip]) -> anyhow::Result<Vec<String>> {
    let mut have = HashSet::new();
    for prerequisite in prerequisites {
        collect_tree_objects(&parse_commit_object(prerequisite)?.tree, &HashSet::new(), &mut have)?;
    }

    let mut ordered: Vec<String> = commits.iter().cloned().collect();
    ordered.sort();
    let mut seen: HashSet<String> = commits.clone();
    for tip in tips {
        let mut object = tip.object.clone();
        while let Ok((kind, content)) = read_object(&object) {
            if kind != "tag" || !seen.insert(object.clone()) {
                break;
            }
            ordered.push(object.clone());
            let text = String::from_utf8_lossy(&content).into_owned();
            match text.lines().next().and_then(|l| l.strip_prefix("object ")) {
                Some(target) => object = target.to_string(),
                None => break,
            }
        }
    }

    let mut trees_and_blobs = HashSet::new();
    let mut sorted_commits: Vec<&String> = commits.iter().collect();
    sorted_commits.sort();
    for commit in sorted_commits {
        collect_tree_objects(&parse_commit_object(commit)?.tree, &have, &mut trees_and_blobs)?;
    }
    let mut rest: Vec<String> = trees_and_blobs.into_iter().filter(|h| !seen.contains(h)).collect();
    rest.sort();
    ordered.extend(rest);
    Ok(ordered)
}

/// Writes a version 2 packfile holding `objects` as whole (undeltified) zlib-compressed
/// entries, ending with the SHA-1 of everything before it.
// Used by `bundle create`.
pub fn write_packfile(objects: &[String]) -> anyhow::Result<Vec<u8>> {
    let mut pack = Vec::new();
    pack.extend_from_slice(b"PACK");
    pack.extend_from_slice(&2u32.to_be_bytes());
    pack.extend_from_slice(&(objects.len() as u32).to_be_bytes());
    for hash in objects {
        let (kind, content) = read_object(hash)?;
        let type_code: u8 = match kind.as_str() {
            "commit" => 1,
            "tree" => 2,
            "blob" => 3,
            "tag" => 4,
            other => anyhow::bail!("fatal: cannot pack object {} of type {}", hash, other),
        };
        let mut size = content.len();
        let mut byte = (type_code << 4) | (size & 0x0f) as u8;
        size >>= 4;
        while size > 0 {
            pack.push(byte | 0x80);
            byte = (size & 0x7f) as u8;
            size >>= 7;
        }
        pack.push(byte);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&content)?;
        pack.extend(encoder.finish()?);
    }
    let checksum: [u8; 20] = Sha1::digest(&pack).into();
    pack.extend_from_slice(&checksum);
    Ok(pack)
}

enum PackEntryData {
    Whole(String, Vec<u8>),
    OffsetDelta(usize, Vec<u8>),
    RefDelta(String, Vec<u8>),
}

/// Reads every object out of a packfile (v2 or v3), checking its trailing checksum and
/// resolving `OFS_DELTA` / `REF_DELTA` entries against bases in the pack or, for thin packs,
/// in this repository. Returns `(type, content)` for each object.
// Used by `bundle unbundle`.
pub fn read_packfile(pack: &[u8]) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let corrupt = || anyhow::anyhow!("fatal: pack has bad object or is truncated");
    if pack.len() < 32 || &pack[..4] != b"PACK" {
        anyhow::bail!("fatal: not a pack file");
    }
    let version = u32::from_be_bytes(pack[4..8].try_into()?);
    if version != 2 && version != 3 {
        anyhow::bail!("fatal: pack version {} unsupported", version);
    }
    let (body, trailer) = pack.split_at(pack.len() - 20);
    if Sha1::digest(body).as_slice() != trailer {
        anyhow::bail!("fatal: pack is corrupted (SHA1 mismatch)");
    }
    let count = u32::from_be_bytes(pack[8..12].try_into()?) as usize;

    let mut entries: Vec<PackEntryData> = Vec::with_capacity(count);
    let mut index_at_offset: HashMap<usize, usize> = HashMap::new();
    let mut pos = 12;
    for _ in 0..count {
        let start = pos;
        let mut byte = *body.get(pos).ok_or_else(corrupt)?;
        pos += 1;
        let type_code = (byte >> 4) & 7;
        while byte & 0x80 != 0 {
            byte = *body.get(pos).ok_or_else(corrupt)?;
            pos += 1;
        }
        let (mut offset_base, mut ref_base) = (None, None);
        match type_code {
            6 => {
                let mut byte = *body.get(pos).ok_or_else(corrupt)?;
                pos += 1;
                let mut offset = (byte & 0x7f) as usize;
                while byte & 0x80 != 0 {
                    byte = *body.get(pos).ok_or_else(corrupt)?;
                    pos += 1;
                    offset = ((offset + 1) << 7) | (byte & 0x7f) as usize;
                }
                offset_base = Some(start.checked_sub(offset).ok_or_else(corrupt)?);
            }
            7 => {
                ref_base = Some(hex::encode(body.get(pos..pos + 20).ok_or_else(corrupt)?));
                pos += 20;
            }
            _ => {}
        }
        let mut decoder = ZlibDecoder::new(body.get(pos..).ok_or_else(corrupt)?);
        let mut data = Vec::new();
        decoder.read_to_end(&mut data).map_err(|_| corrupt())?;
        pos += decoder.total_in() as usize;

        index_at_offset.insert(start, entries.len());
        entries.push(match (type_code, offset_base, ref_base) {
            (1, _, _) => PackEntryData::Whole("commit".into(), data),
            (2, _, _) => PackEntryData::Whole("tree".into(), data),
            (3, _, _) => PackEntryData::Whole("blob".into(), data),
            (4, _, _) => PackEntryData::Whole("tag".into(), data),
            (6, Some(offset), _) => PackEntryData::OffsetDelta(offset, data),
            (7, _, Some(hash)) => PackEntryData::RefDelta(hash, data),
            _ => return Err(corrupt()),
        });
    }

    let mut resolved: Vec<Option<(String, Vec<u8>)>> = vec![None; entries.len()];
    let mut by_hash: HashMap<String, usize> = HashMap::new();
    let object_hash = |kind: &str, content: &[u8]| hex::encode(crate::helpers::hash_content(kind, content));
    loop {
        let mut progress = false;
        for i in 0..entries.len() {
            if resolved[i].is_some() {
                continue;
            }
            let object = match &entries[i] {
                PackEntryData::Whole(kind, data) => Some((kind.clone(), data.clone())),
                PackEntryData::OffsetDelta(offset, delta) => {
                    let base = index_at_offset.get(offset).and_then(|&b| resolved[b].as_ref());
                    match base {
                        Some((kind, base)) => Some((kind.clone(), apply_git_delta(base, delta).ok_or_else(corrupt)?)),
                        None => None,
                    }
                }
                PackEntryData::RefDelta(hash, delta) => {
                    let base = match by_hash.get(hash).and_then(|&b| resolved[b].clone()) {
                        Some(base) => Some(base),
                        None => read_object(hash).ok(),
                    };
                    match base {
                        Some((kind, base)) => Some((kind, apply_git_delta(&base, delta).ok_or_else(corrupt)?)),
                        None => None,
                    }
                }
            };
            if let Some((kind, content)) = object {
                by_hash.insert(object_hash(&kind, &content), i);
                resolved[i] = Some((kind, content));
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    if resolved.iter().any(Option::is_none) {
        anyhow::bail!("fatal: pack has unresolved deltas");
    }
    Ok(resolved.into_iter().flatten().collect())
}

/// Builds the text of a bundle header for the given refs and prerequisites; v3 adds the
/// `@object-format=sha1` capability.
// Used by `bundle create`.
pub fn format_bundle_header(version: u8, refs: &[(String, String)], prerequisites: &[String]) -> anyhow::Result<String> {
    let mut header = format!("# v{} git bundle\n", version);
    if version == 3 {
        header.push_str("@object-format=sha1\n");
    }
    for prerequisite in prerequisites {
        let subject = split_commit_message(&parse_commit_object(prerequisite)?.message).0;
        header.push_str(&format!("-{} {}\n", prerequisite, subject));
    }
    for (oid, name) in refs {
        header.push_str(&format!("{} {}\n", oid, name));
    }
    header.push('\n');
    Ok(header)
}

/// Stores objects in the repository as loose objects, returning how many there were.
// Used by `bundle unbundle`.
pub fn store_loose_objects(objects: &[(String, Vec<u8>)]) -> anyhow::Result<usize> {
    for (kind, content) in objects {
        write_object(kind, content)?;
    }
    Ok(objects.len())
}
