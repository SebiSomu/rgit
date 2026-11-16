use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, bail, Result};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};

use crate::helpers::apply::apply_git_delta;
use crate::helpers::objects::{hash_content, read_object};
use crate::objects::{BuiltPack, PackBuildOptions, PackEntry, PackIndexData, PackedObject, RepoPack};

/// Parses a pack index file, version 2 (`\377tOc` magic) or the older version 1, checking
/// its trailing checksum.
// Used by `repo_packs`, `show-index`, `verify-pack`, `index-pack --verify` and `multi-pack-index`.
pub fn parse_pack_index(data: &[u8]) -> Result<PackIndexData> {
    let truncated = || anyhow!("fatal: index file is truncated or corrupt");
    if data.len() < 4 * 256 + 40 {
        return Err(truncated());
    }
    let (body, trailer) = data.split_at(data.len() - 20);
    if Sha1::digest(body).as_slice() != trailer {
        bail!("fatal: index file checksum does not match its contents");
    }
    let be32 = |at: usize| -> Result<u32> { Ok(u32::from_be_bytes(data.get(at..at + 4).ok_or_else(truncated)?.try_into()?)) };
    let mut index = PackIndexData::default();
    if data[..4] == [0xff, b't', b'O', b'c'] {
        index.version = be32(4)?;
        if index.version != 2 {
            bail!("fatal: unsupported pack index version {}", index.version);
        }
        let count = be32(8 + 255 * 4)? as usize;
        let names = 8 + 256 * 4;
        let crcs = names + count * 20;
        let offsets = crcs + count * 4;
        let large = offsets + count * 4;
        for i in 0..count {
            let mut hash = [0u8; 20];
            hash.copy_from_slice(data.get(names + i * 20..names + i * 20 + 20).ok_or_else(truncated)?);
            index.hashes.push(hash);
            index.crcs.push(be32(crcs + i * 4)?);
            let offset = be32(offsets + i * 4)?;
            if offset & 0x8000_0000 != 0 {
                let at = large + (offset & 0x7fff_ffff) as usize * 8;
                index.offsets.push(u64::from_be_bytes(data.get(at..at + 8).ok_or_else(truncated)?.try_into()?));
            } else {
                index.offsets.push(offset as u64);
            }
        }
    } else {
        index.version = 1;
        let count = be32(255 * 4)? as usize;
        for i in 0..count {
            let at = 256 * 4 + i * 24;
            index.offsets.push(be32(at)? as u64);
            let mut hash = [0u8; 20];
            hash.copy_from_slice(data.get(at + 4..at + 24).ok_or_else(truncated)?);
            index.hashes.push(hash);
        }
    }
    index.pack_checksum.copy_from_slice(&data[data.len() - 40..data.len() - 20]);
    Ok(index)
}

/// The packs of the repository, cached for the process.
static REPO_PACKS: RwLock<Option<Arc<Vec<RepoPack>>>> = RwLock::new(None);

/// Every pack in `.git/objects/pack` that has both a `.pack` and a readable `.idx`, sorted by
/// name. The list is read once and cached; `forget_repo_packs` drops the cache.
// Used by `read_packed_object`, `packed_object_names`, `repack`, `prune-packed`, `fsck`, `multi-pack-index` and `commit-graph`.
pub fn repo_packs() -> Arc<Vec<RepoPack>> {
    if let Some(packs) = REPO_PACKS.read().ok().and_then(|guard| guard.clone()) {
        return packs;
    }
    let packs = packs_in_dir(&crate::repo::path(".git/objects/pack"));
    let packs = Arc::new(packs);
    if let Ok(mut guard) = REPO_PACKS.write() {
        *guard = Some(packs.clone());
    }
    packs
}

/// Loads every pack of a pack directory that has both a `.pack` and a readable `.idx`,
/// sorted by name.
// Used by `repo_packs` and `read_object_in_objects_dir`.
pub fn packs_in_dir(dir: &Path) -> Vec<RepoPack> {
    let mut packs = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "idx") {
                continue;
            }
            let pack_path = path.with_extension("pack");
            if !pack_path.is_file() {
                continue;
            }
            let Ok(index) = fs::read(&path).map_err(anyhow::Error::from).and_then(|bytes| parse_pack_index(&bytes)) else { continue };
            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            packs.push(RepoPack { name, pack_path, index, data: std::sync::OnceLock::new() });
        }
    }
    packs.sort_by(|a, b| a.name.cmp(&b.name));
    packs
}

/// Reads an object from another repository's object directory (`<git-dir>/objects`): a
/// loose file there, or else one of its packs. The other repository's packs are loaded once
/// per process.
// Used by `fetch`, `push` and `clone` over the local transport (`read_object_from_path`).
pub fn read_object_in_objects_dir(objects_dir: &Path, hash: &str) -> Option<(String, Vec<u8>)> {
    static OTHER_PACKS: std::sync::Mutex<Option<HashMap<PathBuf, Arc<Vec<RepoPack>>>>> = std::sync::Mutex::new(None);
    if hash.len() != 40 {
        return None;
    }
    if let Ok(compressed) = fs::read(objects_dir.join(&hash[..2]).join(&hash[2..])) {
        let mut raw = Vec::new();
        ZlibDecoder::new(&compressed[..]).read_to_end(&mut raw).ok()?;
        let nul = raw.iter().position(|b| *b == 0)?;
        let kind = String::from_utf8_lossy(&raw[..nul]).split(' ').next()?.to_string();
        return Some((kind, raw[nul + 1..].to_vec()));
    }
    let packs = {
        let mut guard = OTHER_PACKS.lock().ok()?;
        let map = guard.get_or_insert_with(HashMap::new);
        map.entry(objects_dir.to_path_buf()).or_insert_with(|| Arc::new(packs_in_dir(&objects_dir.join("pack")))).clone()
    };
    let mut wanted = [0u8; 20];
    hex::decode_to_slice(hash, &mut wanted).ok()?;
    let lookup = |base: &str| read_object_in_objects_dir(objects_dir, base);
    for pack in packs.iter() {
        if let Some(position) = find_in_index(&pack.index, &wanted) {
            if let Ok(object) = read_object_at(pack_bytes(pack), pack.index.offsets[position], 0, &lookup) {
                return Some(object);
            }
        }
    }
    None
}

/// Writes an object as a loose file into another repository's object directory.
// Used by `push` over the local transport when the object is packed here.
pub fn write_loose_object_in(objects_dir: &Path, kind: &str, content: &[u8]) -> Result<String> {
    let mut store = format!("{} {}\0", kind, content.len()).into_bytes();
    store.extend_from_slice(content);
    let hash = hex::encode(Sha1::digest(&store));
    let dir = objects_dir.join(&hash[..2]);
    let path = dir.join(&hash[2..]);
    if !path.exists() {
        fs::create_dir_all(&dir)?;
        fs::write(&path, deflate(&store)?)?;
    }
    Ok(hash)
}

/// Drops the cached pack list, after packs were added or removed.
// Used by `index-pack`, `repack`, `multi-pack-index expire` and `fast-import`.
pub fn forget_repo_packs() {
    if let Ok(mut guard) = REPO_PACKS.write() {
        *guard = None;
    }
}

/// The bytes of a pack, read from disk on first use.
// Used by `read_packed_object` and `verify-pack`.
pub fn pack_bytes(pack: &RepoPack) -> &[u8] {
    pack.data.get_or_init(|| fs::read(&pack.pack_path).unwrap_or_default())
}

/// Position of an object name in an index, by binary search.
// Used by `read_packed_object`, `packed_object_exists` and `multi-pack-index`.
pub fn find_in_index(index: &PackIndexData, hash: &[u8; 20]) -> Option<usize> {
    index.hashes.binary_search(hash).ok()
}

/// Reads an object from the repository's packs, resolving delta chains. `None` when no pack
/// has it (or the pack is damaged).
// Used by `read_object` as the fallback for objects that are not loose.
pub fn read_packed_object(hash: &str) -> Option<(String, Vec<u8>)> {
    let mut wanted = [0u8; 20];
    hex::decode_to_slice(hash, &mut wanted).ok()?;
    let packs = repo_packs();
    for pack in packs.iter() {
        if let Some(position) = find_in_index(&pack.index, &wanted) {
            if let Ok(object) = read_object_at(pack_bytes(pack), pack.index.offsets[position], 0, &|base| read_object(base).ok()) {
                return Some(object);
            }
        }
    }
    None
}

/// Whether some pack lists the object.
// Used by `prune-packed`, `repack` and `pack-objects --incremental`.
pub fn packed_object_exists(hash: &str) -> bool {
    let mut wanted = [0u8; 20];
    if hex::decode_to_slice(hash, &mut wanted).is_err() {
        return false;
    }
    repo_packs().iter().any(|pack| find_in_index(&pack.index, &wanted).is_some())
}

/// Names of all packed objects starting with the hex `prefix`, without duplicates.
// Used by short-hash resolution (`rev-parse`) and hash abbreviation.
pub fn packed_object_names(prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for pack in repo_packs().iter() {
        // Hashes are sorted, so the matches form one run starting at the first name >= prefix.
        let start = pack.index.hashes.partition_point(|h| hex::encode(h).as_str() < prefix);
        for hash in &pack.index.hashes[start..] {
            let name = hex::encode(hash);
            if !name.starts_with(prefix) {
                break;
            }
            names.push(name);
        }
    }
    names.sort();
    names.dedup();
    names
}

/// The object type name for a pack type code.
// Used by `read_object_at` and `resolve_pack_entries`.
fn type_name(code: u8) -> Option<&'static str> {
    match code {
        1 => Some("commit"),
        2 => Some("tree"),
        3 => Some("blob"),
        4 => Some("tag"),
        _ => None,
    }
}

/// The pack type code of an object type name.
// Used by `build_pack` and `index-pack --fix-thin`.
pub fn type_code(kind: &str) -> Result<u8> {
    match kind {
        "commit" => Ok(1),
        "tree" => Ok(2),
        "blob" => Ok(3),
        "tag" => Ok(4),
        other => bail!("fatal: cannot pack an object of type {}", other),
    }
}

/// Inflates the zlib stream starting at `pos`; returns the data and how many compressed
/// bytes it took.
// Used by `parse_entry_at`.
fn inflate_at(pack: &[u8], pos: usize) -> Result<(Vec<u8>, usize)> {
    let mut decoder = ZlibDecoder::new(pack.get(pos..).ok_or_else(|| anyhow!("fatal: pack is truncated"))?);
    let mut data = Vec::new();
    decoder.read_to_end(&mut data).map_err(|_| anyhow!("fatal: pack has a corrupt zlib stream at offset {}", pos))?;
    Ok((data, decoder.total_in() as usize))
}

/// Parses the pack entry at `offset`: the type-and-size header (4 bits of type, size in
/// 4 + 7·n bits), the base reference of a delta (a backwards offset for type 6, a 20-byte
/// object name for type 7) and the inflated data.
// Used by `read_object_at` and `scan_pack`.
pub fn parse_entry_at(pack: &[u8], offset: u64) -> Result<PackEntry> {
    let truncated = || anyhow!("fatal: pack is truncated");
    let mut pos = offset as usize;
    let mut byte = *pack.get(pos).ok_or_else(truncated)?;
    pos += 1;
    let type_code = (byte >> 4) & 7;
    let mut size = (byte & 0x0f) as u64;
    let mut shift = 4;
    while byte & 0x80 != 0 {
        byte = *pack.get(pos).ok_or_else(truncated)?;
        pos += 1;
        size |= ((byte & 0x7f) as u64) << shift;
        shift += 7;
    }
    let (mut base_offset, mut base_hash) = (None, None);
    match type_code {
        6 => {
            let mut byte = *pack.get(pos).ok_or_else(truncated)?;
            pos += 1;
            let mut distance = (byte & 0x7f) as u64;
            while byte & 0x80 != 0 {
                byte = *pack.get(pos).ok_or_else(truncated)?;
                pos += 1;
                distance = ((distance + 1) << 7) | (byte & 0x7f) as u64;
            }
            base_offset = Some(offset.checked_sub(distance).ok_or_else(|| anyhow!("fatal: delta base offset is out of bound"))?);
        }
        7 => {
            base_hash = Some(hex::encode(pack.get(pos..pos + 20).ok_or_else(truncated)?));
            pos += 20;
        }
        1..=4 => {}
        other => bail!("fatal: pack has an unknown object type {} at offset {}", other, offset),
    }
    let (data, consumed) = inflate_at(pack, pos)?;
    Ok(PackEntry { offset, end: (pos + consumed) as u64, type_code, size, base_offset, base_hash, data })
}

/// Reads the object stored at `offset` of a pack, applying deltas down to the whole base.
/// A ref delta's base is found through `lookup` (anywhere in the same repository).
// Used by `read_packed_object` and `read_object_in_objects_dir`.
fn read_object_at(pack: &[u8], offset: u64, depth: u32, lookup: &dyn Fn(&str) -> Option<(String, Vec<u8>)>) -> Result<(String, Vec<u8>)> {
    if depth > 4096 {
        bail!("fatal: delta chain is too deep");
    }
    let entry = parse_entry_at(pack, offset)?;
    if let Some(kind) = type_name(entry.type_code) {
        return Ok((kind.to_string(), entry.data));
    }
    let (kind, base) = match (entry.base_offset, &entry.base_hash) {
        (Some(base_offset), _) => read_object_at(pack, base_offset, depth + 1, lookup)?,
        (_, Some(base_hash)) => lookup(base_hash).ok_or_else(|| anyhow!("fatal: delta base {} is missing", base_hash))?,
        _ => bail!("fatal: delta without a base at offset {}", offset),
    };
    let content = apply_git_delta(&base, &entry.data).ok_or_else(|| anyhow!("fatal: corrupt delta at offset {}", offset))?;
    Ok((kind, content))
}

/// Checks a pack's framing — `PACK` signature, version 2 or 3, trailing SHA-1 — and reads
/// all entries in order. Returns the entries and the trailer checksum.
// Used by `index-pack`, `verify-pack` and `unpack-objects`.
pub fn scan_pack(pack: &[u8]) -> Result<(Vec<PackEntry>, [u8; 20])> {
    if pack.len() < 32 || &pack[..4] != b"PACK" {
        bail!("fatal: pack signature mismatch");
    }
    let version = u32::from_be_bytes(pack[4..8].try_into()?);
    if version != 2 && version != 3 {
        bail!("fatal: pack version {} unsupported", version);
    }
    let (body, trailer) = pack.split_at(pack.len() - 20);
    if Sha1::digest(body).as_slice() != trailer {
        bail!("fatal: pack is corrupted (SHA1 mismatch)");
    }
    let count = u32::from_be_bytes(pack[8..12].try_into()?) as usize;
    let mut entries = Vec::with_capacity(count);
    let mut offset = 12u64;
    for _ in 0..count {
        let entry = parse_entry_at(body, offset)?;
        offset = entry.end;
        entries.push(entry);
    }
    if offset as usize != body.len() {
        bail!("fatal: pack has junk at the end");
    }
    let mut checksum = [0u8; 20];
    checksum.copy_from_slice(trailer);
    Ok((entries, checksum))
}

/// Turns scanned entries into objects: whole entries directly, deltas by applying them to
/// their base, which is another entry of the pack or — for a thin pack, when
/// `allow_external` — an object of the repository. Returns the objects in pack order and
/// the names of the external bases that were needed.
// Used by `index-pack`, `verify-pack` and `unpack-objects`.
pub fn resolve_pack_entries(pack: &[u8], entries: &[PackEntry], allow_external: bool) -> Result<(Vec<PackedObject>, Vec<String>)> {
    let by_offset: HashMap<u64, usize> = entries.iter().enumerate().map(|(i, e)| (e.offset, i)).collect();
    let mut resolved: Vec<Option<PackedObject>> = vec![None; entries.len()];
    let mut by_hash: HashMap<String, usize> = HashMap::new();
    let mut external: Vec<String> = Vec::new();
    let finish = |entry: &PackEntry, kind: &str, content: Vec<u8>, depth: u32, base: Option<String>| PackedObject {
        hash: hex::encode(hash_content(kind, &content)),
        kind: kind.to_string(),
        content,
        offset: entry.offset,
        end: entry.end,
        header_size: entry.size,
        crc: {
            let mut crc = flate2::Crc::new();
            crc.update(&pack[entry.offset as usize..entry.end as usize]);
            crc.sum()
        },
        depth,
        base,
    };

    let mut pending: Vec<usize> = (0..entries.len()).collect();
    let mut use_external = false;
    while !pending.is_empty() {
        let mut still_pending = Vec::new();
        for &i in &pending {
            let entry = &entries[i];
            let object = if let Some(kind) = type_name(entry.type_code) {
                Some(finish(entry, kind, entry.data.clone(), 0, None))
            } else {
                let base: Option<(String, Vec<u8>, u32, String)> = match (entry.base_offset, &entry.base_hash) {
                    (Some(offset), _) => {
                        let index = by_offset.get(&offset).ok_or_else(|| anyhow!("fatal: delta base offset is out of bound"))?;
                        resolved[*index].as_ref().map(|b| (b.kind.clone(), b.content.clone(), b.depth, b.hash.clone()))
                    }
                    (_, Some(hash)) => match by_hash.get(hash).and_then(|index| resolved[*index].as_ref()) {
                        Some(b) => Some((b.kind.clone(), b.content.clone(), b.depth, b.hash.clone())),
                        None if use_external => read_object(hash).ok().map(|(kind, content)| {
                            if !external.contains(hash) {
                                external.push(hash.clone());
                            }
                            (kind, content, 0, hash.clone())
                        }),
                        None => None,
                    },
                    _ => None,
                };
                match base {
                    Some((kind, base_content, depth, base_hash)) => {
                        let content = apply_git_delta(&base_content, &entry.data).ok_or_else(|| anyhow!("fatal: corrupt delta at offset {}", entry.offset))?;
                        Some(finish(entry, &kind, content, depth + 1, Some(base_hash)))
                    }
                    None => None,
                }
            };
            match object {
                Some(object) => {
                    by_hash.insert(object.hash.clone(), i);
                    resolved[i] = Some(object);
                }
                None => still_pending.push(i),
            }
        }
        if still_pending.len() == pending.len() {
            // No progress inside the pack: the remaining bases must come from outside.
            if allow_external && !use_external {
                use_external = true;
            } else {
                bail!("fatal: pack has {} unresolved delta{}", still_pending.len(), if still_pending.len() == 1 { "" } else { "s" });
            }
        }
        pending = still_pending;
    }
    Ok((resolved.into_iter().flatten().collect(), external))
}

/// Encodes a pack entry header: type in bits 4-6 of the first byte, size in the low 4 bits
/// and then 7 bits per following byte, each byte but the last flagged with 0x80.
// Used by `build_pack` and `index-pack --fix-thin`.
pub fn entry_header(type_code: u8, size: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut size = size;
    let mut byte = (type_code << 4) | (size & 0x0f) as u8;
    size >>= 4;
    while size > 0 {
        out.push(byte | 0x80);
        byte = (size & 0x7f) as u8;
        size >>= 7;
    }
    out.push(byte);
    out
}

/// Encodes the backwards distance of an offset delta in git's big-endian base-128 form.
// Used by `build_pack`.
fn offset_delta_distance(mut distance: u64) -> Vec<u8> {
    let mut bytes = vec![(distance & 0x7f) as u8];
    distance >>= 7;
    while distance > 0 {
        distance -= 1;
        bytes.push(0x80 | (distance & 0x7f) as u8);
        distance >>= 7;
    }
    bytes.reverse();
    bytes
}

/// zlib-compresses data.
// Used by `build_pack` and `index-pack --fix-thin`.
pub fn deflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

/// Encodes a size as the little-endian base-128 varint a delta starts with.
// Used by `create_delta`.
fn delta_varint(out: &mut Vec<u8>, mut value: usize) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// Builds git delta instructions that turn `base` into `target`: the two sizes, then `copy`
/// commands (offset and length in the base) for runs found in the base and `insert`
/// commands (up to 127 literal bytes) for the rest. Runs are found through an index of the
/// base's 16-byte blocks. Returns `None` when the delta would not be smaller than
/// `max_size`.
// Used by `build_pack`.
pub fn create_delta(base: &[u8], target: &[u8], max_size: usize) -> Option<Vec<u8>> {
    const BLOCK: usize = 16;
    if base.len() < BLOCK || target.len() < BLOCK {
        return None;
    }
    let mut blocks: HashMap<&[u8], Vec<usize>> = HashMap::new();
    let mut at = 0;
    while at + BLOCK <= base.len() {
        let bucket = blocks.entry(&base[at..at + BLOCK]).or_default();
        if bucket.len() < 8 {
            bucket.push(at);
        }
        at += BLOCK;
    }

    let mut out = Vec::new();
    delta_varint(&mut out, base.len());
    delta_varint(&mut out, target.len());
    let mut literal: Vec<u8> = Vec::new();
    let flush = |out: &mut Vec<u8>, literal: &mut Vec<u8>| {
        for chunk in literal.chunks(127) {
            out.push(chunk.len() as u8);
            out.extend_from_slice(chunk);
        }
        literal.clear();
    };

    let mut pos = 0;
    while pos < target.len() {
        let mut best: Option<(usize, usize)> = None;
        if pos + BLOCK <= target.len() {
            if let Some(candidates) = blocks.get(&target[pos..pos + BLOCK]) {
                for &start in candidates {
                    let mut length = BLOCK;
                    while start + length < base.len() && pos + length < target.len() && base[start + length] == target[pos + length] {
                        length += 1;
                    }
                    if best.is_none_or(|(_, best_length)| length > best_length) {
                        best = Some((start, length));
                    }
                }
            }
        }
        let Some((mut start, mut length)) = best else {
            literal.push(target[pos]);
            pos += 1;
            continue;
        };
        // Extend the match backwards over bytes that were queued as literals.
        while start > 0 && !literal.is_empty() && base[start - 1] == *literal.last().unwrap_or(&0) {
            literal.pop();
            start -= 1;
            pos -= 1;
            length += 1;
        }
        flush(&mut out, &mut literal);
        let mut remaining = length;
        let mut from = start;
        while remaining > 0 {
            let piece = remaining.min(0xff_ffff);
            let mut command = 0x80u8;
            let mut operands = Vec::new();
            for bit in 0..4 {
                let byte = ((from >> (8 * bit)) & 0xff) as u8;
                if byte != 0 {
                    command |= 1 << bit;
                    operands.push(byte);
                }
            }
            for bit in 0..3 {
                let byte = ((piece >> (8 * bit)) & 0xff) as u8;
                if byte != 0 {
                    command |= 0x10 << bit;
                    operands.push(byte);
                }
            }
            out.push(command);
            out.extend(operands);
            from += piece;
            remaining -= piece;
        }
        pos += length;
        if out.len() >= max_size {
            return None;
        }
    }
    flush(&mut out, &mut literal);
    (out.len() < max_size).then_some(out)
}

/// Builds a version 2 pack of the named objects, in the given order. With a delta window,
/// each tree or blob is tried as a delta against the previous `window` objects of its type
/// (the smallest delta wins if it is under half the object's size and the chain stays within
/// `depth`), stored as an offset delta; everything else is stored whole.
// Used by `pack-objects` and `repack`.
pub fn build_pack(objects: &[String], opts: &PackBuildOptions) -> Result<BuiltPack> {
    let mut built = BuiltPack::default();
    built.bytes.extend_from_slice(b"PACK");
    built.bytes.extend_from_slice(&2u32.to_be_bytes());
    built.bytes.extend_from_slice(&(objects.len() as u32).to_be_bytes());

    // Recently written objects per type: (offset, content, chain depth).
    let mut recent: HashMap<String, Vec<(u64, Arc<Vec<u8>>, u32)>> = HashMap::new();
    for name in objects {
        let (kind, content) = read_object(name)?;
        let code = type_code(&kind)?;
        let offset = built.bytes.len() as u64;
        let content = Arc::new(content);
        let candidates = recent.entry(kind.clone()).or_default();

        let mut best: Option<(u64, Vec<u8>, u32)> = None;
        if opts.window > 0 && (kind == "blob" || kind == "tree") {
            for (base_offset, base, base_depth) in candidates.iter().rev() {
                if *base_depth >= opts.depth {
                    continue;
                }
                let limit = best.as_ref().map(|(_, delta, _)| delta.len()).unwrap_or(content.len() / 2);
                if let Some(delta) = create_delta(base, &content, limit) {
                    best = Some((*base_offset, delta, *base_depth + 1));
                }
            }
        }
        let depth = match best {
            Some((base_offset, delta, depth)) => {
                built.bytes.extend(entry_header(6, delta.len()));
                built.bytes.extend(offset_delta_distance(offset - base_offset));
                built.bytes.extend(deflate(&delta)?);
                built.deltas += 1;
                depth
            }
            None => {
                built.bytes.extend(entry_header(code, content.len()));
                built.bytes.extend(deflate(&content)?);
                0
            }
        };
        let mut crc = flate2::Crc::new();
        crc.update(&built.bytes[offset as usize..]);
        let mut hash = [0u8; 20];
        hex::decode_to_slice(name, &mut hash)?;
        built.entries.push((hash, offset, crc.sum()));

        candidates.push((offset, content, depth));
        if candidates.len() > opts.window.max(1) {
            candidates.remove(0);
        }
    }
    let checksum: [u8; 20] = Sha1::digest(&built.bytes).into();
    built.bytes.extend_from_slice(&checksum);
    Ok(built)
}

/// Serializes a version 2 pack index: magic and version, the 256-entry fan-out table
/// (how many names start with a byte ≤ n), the sorted names, their CRC-32s, their 31-bit
/// offsets (larger ones go to a 64-bit table), the pack checksum and the index's own
/// checksum.
// Used by `index-pack`, `pack-objects` and `repack`.
pub fn write_pack_index(entries: &[([u8; 20], u64, u32)], pack_checksum: &[u8; 20]) -> Vec<u8> {
    let mut sorted: Vec<&([u8; 20], u64, u32)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = vec![0xff, b't', b'O', b'c', 0, 0, 0, 2];
    let mut fanout = [0u32; 256];
    for entry in &sorted {
        fanout[entry.0[0] as usize] += 1;
    }
    let mut running = 0u32;
    for count in fanout {
        running += count;
        out.extend_from_slice(&running.to_be_bytes());
    }
    for entry in &sorted {
        out.extend_from_slice(&entry.0);
    }
    for entry in &sorted {
        out.extend_from_slice(&entry.2.to_be_bytes());
    }
    let mut large: Vec<u64> = Vec::new();
    for entry in &sorted {
        if entry.1 < 0x8000_0000 {
            out.extend_from_slice(&(entry.1 as u32).to_be_bytes());
        } else {
            out.extend_from_slice(&(0x8000_0000u32 | large.len() as u32).to_be_bytes());
            large.push(entry.1);
        }
    }
    for offset in large {
        out.extend_from_slice(&offset.to_be_bytes());
    }
    out.extend_from_slice(pack_checksum);
    let checksum: [u8; 20] = Sha1::digest(&out).into();
    out.extend_from_slice(&checksum);
    out
}

/// Writes a pack and its index into a directory as `pack-<checksum>.pack` / `.idx` (read-only,
/// like git's) unless that pack is already there. Returns the checksum in hex.
// Used by `pack-objects`, `repack` and `index-pack --stdin`.
pub fn store_pack(dir: &Path, prefix: &str, pack: &[u8], index: &[u8]) -> Result<String> {
    let checksum = hex::encode(&pack[pack.len() - 20..]);
    fs::create_dir_all(dir)?;
    let base: PathBuf = dir.join(format!("{}{}", prefix, checksum));
    let write = |path: PathBuf, bytes: &[u8]| -> Result<()> {
        if path.exists() {
            let mut permissions = fs::metadata(&path)?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(&path, permissions)?;
        }
        fs::write(&path, bytes)?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions)?;
        Ok(())
    };
    write(PathBuf::from(format!("{}.pack", base.display())), pack)?;
    write(PathBuf::from(format!("{}.idx", base.display())), index)?;
    forget_repo_packs();
    Ok(checksum)
}

/// Lists every object reachable from `tips` (commits, tags, trees or blobs) that is not in
/// `excluded`: commits and tags first, in walk order, then their trees and blobs. Missing
/// objects are skipped.
// Used by `pack-objects --revs` / `--all`, `repack -a` and `fast-export`.
pub fn reachable_objects(tips: &[String], excluded: &HashSet<String>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut commits: Vec<String> = Vec::new();
    let mut trees: Vec<String> = Vec::new();
    let mut stack: Vec<String> = tips.iter().rev().cloned().collect();
    while let Some(hash) = stack.pop() {
        if excluded.contains(&hash) || !seen.insert(hash.clone()) {
            continue;
        }
        let Ok((kind, content)) = read_object(&hash) else {
            seen.remove(&hash);
            continue;
        };
        let text = String::from_utf8_lossy(&content);
        match kind.as_str() {
            "commit" => {
                commits.push(hash);
                let header: Vec<&str> = text.lines().take_while(|l| !l.is_empty()).collect();
                if let Some(tree) = header.iter().find_map(|l| l.strip_prefix("tree ")) {
                    trees.push(tree.trim().to_string());
                }
                for parent in header.iter().rev().filter_map(|l| l.strip_prefix("parent ")) {
                    stack.push(parent.trim().to_string());
                }
            }
            "tag" => {
                commits.push(hash);
                if let Some(target) = text.lines().take_while(|l| !l.is_empty()).find_map(|l| l.strip_prefix("object ")) {
                    stack.push(target.trim().to_string());
                }
            }
            _ => {
                seen.remove(&hash);
                trees.push(hash);
            }
        }
    }

    let mut rest: Vec<String> = Vec::new();
    let mut stack: Vec<String> = trees.into_iter().rev().collect();
    while let Some(hash) = stack.pop() {
        if excluded.contains(&hash) || !seen.insert(hash.clone()) {
            continue;
        }
        let Ok((kind, content)) = read_object(&hash) else {
            seen.remove(&hash);
            continue;
        };
        rest.push(hash);
        if kind != "tree" {
            continue;
        }
        let mut children = Vec::new();
        let mut pos = 0;
        while pos < content.len() {
            let Some(space) = content[pos..].iter().position(|b| *b == b' ').map(|i| pos + i) else { break };
            let Some(nul) = content[space..].iter().position(|b| *b == 0).map(|i| space + i) else { break };
            let Some(child) = content.get(nul + 1..nul + 21) else { break };
            // Submodule links (mode 160000) point at commits of another repository.
            if &content[pos..space] != b"160000" {
                children.push(hex::encode(child));
            }
            pos = nul + 21;
        }
        stack.extend(children.into_iter().rev());
    }
    commits.extend(rest);
    commits
}

/// Resolves the argument of `verify-pack` / `index-pack` to the pack file and index file it
/// stands for: a `.pack`, an `.idx`, or their common base name.
// Used by `verify-pack`.
pub fn pack_and_index_paths(arg: &str) -> (PathBuf, PathBuf) {
    let base = arg.strip_suffix(".pack").or_else(|| arg.strip_suffix(".idx")).unwrap_or(arg);
    (PathBuf::from(format!("{}.pack", base)), PathBuf::from(format!("{}.idx", base)))
}

/// Makes a thin pack self-contained: appends the given external base objects as whole
/// entries, fixes the object count in the header and recomputes the trailing checksum.
// Used by `index-pack --fix-thin`.
pub fn complete_thin_pack(pack: &[u8], external: &[String]) -> Result<Vec<u8>> {
    let mut out = pack[..pack.len() - 20].to_vec();
    for name in external {
        let (kind, content) = read_object(name)?;
        out.extend(entry_header(type_code(&kind)?, content.len()));
        out.extend(deflate(&content)?);
    }
    let count = u32::from_be_bytes(out[8..12].try_into()?) + external.len() as u32;
    out[8..12].copy_from_slice(&count.to_be_bytes());
    let checksum: [u8; 20] = Sha1::digest(&out).into();
    out.extend_from_slice(&checksum);
    Ok(out)
}

/// Scans and resolves a whole pack and builds its index. With `fix_thin`, deltas against
/// objects outside the pack are allowed and the pack is completed with those objects first.
/// Returns the (possibly completed) pack bytes, the index bytes and the objects.
// Used by `index-pack`, `unpack-objects` (objects only) and `verify-pack`.
pub fn index_pack_bytes(pack: Vec<u8>, fix_thin: bool) -> Result<(Vec<u8>, Vec<u8>, Vec<PackedObject>)> {
    let (entries, _) = scan_pack(&pack)?;
    let (mut objects, external) = resolve_pack_entries(&pack, &entries, fix_thin)?;
    let mut pack = pack;
    if !external.is_empty() {
        pack = complete_thin_pack(&pack, &external)?;
        let (entries, _) = scan_pack(&pack)?;
        objects = resolve_pack_entries(&pack, &entries, false)?.0;
    }
    let mut checksum = [0u8; 20];
    checksum.copy_from_slice(&pack[pack.len() - 20..]);
    let mut listed: Vec<([u8; 20], u64, u32)> = Vec::with_capacity(objects.len());
    for object in &objects {
        let mut hash = [0u8; 20];
        hex::decode_to_slice(&object.hash, &mut hash)?;
        listed.push((hash, object.offset, object.crc));
    }
    let index = write_pack_index(&listed, &checksum);
    Ok((pack, index, objects))
}

/// Removes the loose copy of every object that is also in a pack. With `dry_run` nothing is
/// removed. Returns the paths (relative to the repository, with forward slashes).
// Used by `prune-packed` and `repack -d`.
pub fn prune_packed_objects(dry_run: bool) -> Result<Vec<String>> {
    let mut pruned = Vec::new();
    for object in crate::helpers::gc::list_loose_objects()? {
        if !packed_object_exists(&object.hash) {
            continue;
        }
        pruned.push(format!(".git/objects/{}/{}", &object.hash[..2], &object.hash[2..]));
        if !dry_run {
            crate::helpers::gc::remove_loose_object(&object)?;
            if let Some(dir) = object.path.parent() {
                let _ = fs::remove_dir(dir);
            }
        }
    }
    Ok(pruned)
}
