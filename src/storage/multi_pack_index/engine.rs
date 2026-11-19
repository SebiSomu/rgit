use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::storage::chunk_file::{assemble_chunk_file, fanout_table, parse_chunk_file, read_oid_chunks};
use crate::core::pack::find_in_index;
use crate::objects::RepoPack;

/// Path of the multi-pack-index file.
// Used by `multi-pack-index`.
pub fn midx_path() -> PathBuf {
    crate::core::repo::path(".git/objects/pack/multi-pack-index")
}

/// Builds a multi-pack-index (version 1, SHA-1) over the given packs: one sorted list of
/// all object names with, for each, the pack that holds it and its offset there. Chunks:
/// `PNAM` (index file names, sorted, NUL-terminated, padded to 4 bytes), `OIDF`, `OIDL`,
/// `OOFF` (pack number and 31-bit offset) and `LOFF` (64-bit offsets, when needed). An
/// object in several packs is taken from the preferred pack if it is there, else from the
/// most recently modified pack. Returns the file and how many objects each pack contributes.
// Used by `multi-pack-index write` / `expire`.
pub fn write_midx(packs: &[&RepoPack], preferred: Option<&str>) -> (Vec<u8>, Vec<usize>) {
    let mut order: Vec<&RepoPack> = packs.to_vec();
    order.sort_by(|a, b| a.name.cmp(&b.name));
    let mtime = |pack: &RepoPack| fs::metadata(&pack.pack_path).and_then(|m| m.modified()).ok();
    let mtimes: Vec<_> = order.iter().map(|p| mtime(p)).collect();
    let is_preferred = |i: usize| preferred.is_some_and(|name| order[i].name == name.trim_end_matches(".idx").trim_end_matches(".pack"));

    // (name, pack number, offset), sorted by name and then by which copy wins.
    let mut entries: Vec<([u8; 20], usize, u64)> = Vec::new();
    for (number, pack) in order.iter().enumerate() {
        for (i, hash) in pack.index.hashes.iter().enumerate() {
            entries.push((*hash, number, pack.index.offsets[i]));
        }
    }
    entries.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(is_preferred(b.1).cmp(&is_preferred(a.1)))
            .then(mtimes[b.1].cmp(&mtimes[a.1]))
            .then(a.1.cmp(&b.1))
    });
    entries.dedup_by(|later, first| later.0 == first.0);

    let mut used = vec![0usize; order.len()];
    let names: Vec<[u8; 20]> = entries.iter().map(|e| e.0).collect();
    let (mut offsets, mut large) = (Vec::new(), Vec::new());
    for (_, number, offset) in &entries {
        used[*number] += 1;
        offsets.extend_from_slice(&(*number as u32).to_be_bytes());
        if *offset > 0x7fff_ffff {
            offsets.extend_from_slice(&(0x8000_0000u32 | (large.len() / 8) as u32).to_be_bytes());
            large.extend_from_slice(&offset.to_be_bytes());
        } else {
            offsets.extend_from_slice(&(*offset as u32).to_be_bytes());
        }
    }
    let mut pack_names = Vec::new();
    for pack in &order {
        pack_names.extend_from_slice(format!("{}.idx", pack.name).as_bytes());
        pack_names.push(0);
    }
    while pack_names.len() % 4 != 0 {
        pack_names.push(0);
    }

    let mut chunks: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"PNAM", pack_names),
        (*b"OIDF", fanout_table(&names)),
        (*b"OIDL", names.iter().flatten().copied().collect()),
        (*b"OOFF", offsets),
    ];
    if !large.is_empty() {
        chunks.push((*b"LOFF", large));
    }
    let mut header = vec![b'M', b'I', b'D', b'X', 1, 1, chunks.len() as u8, 0];
    header.extend_from_slice(&(order.len() as u32).to_be_bytes());
    (assemble_chunk_file(&header, &chunks), used)
}

/// Parses a multi-pack-index: the pack index names it covers and, per object, name, pack
/// number and offset.
// Used by `verify_midx` and `multi-pack-index expire`.
pub fn parse_midx(data: &[u8]) -> Result<(Vec<String>, Vec<([u8; 20], u32, u64)>)> {
    if data.len() < 12 || &data[..4] != b"MIDX" {
        bail!("multi-pack-index signature does not match signature 0x4d494458");
    }
    if data[4] != 1 {
        bail!("multi-pack-index version {} not recognized", data[4]);
    }
    if data[5] != 1 {
        bail!("multi-pack-index hash version {} does not match version 1", data[5]);
    }
    let pack_count = u32::from_be_bytes(data[8..12].try_into()?) as usize;
    let chunks = parse_chunk_file(data, 12, data[6] as usize)?;
    let find = |id: &[u8; 4], what: &str| chunks.iter().find(|(chunk, _)| chunk == id).map(|(_, data)| *data).ok_or_else(|| anyhow::anyhow!("multi-pack-index required {} chunk missing or corrupted", what));
    let pack_names: Vec<String> = find(b"PNAM", "pack-name")?.split(|b| *b == 0).filter(|name| !name.is_empty()).map(|name| String::from_utf8_lossy(name).to_string()).collect();
    if pack_names.len() != pack_count {
        bail!("multi-pack-index pack-name chunk lists {} packs but the header says {}", pack_names.len(), pack_count);
    }
    if pack_names.windows(2).any(|pair| pair[0] >= pair[1]) {
        bail!("multi-pack-index pack names out of order");
    }
    let names = read_oid_chunks(find(b"OIDF", "OID fanout")?, find(b"OIDL", "OID lookup")?)?;
    let offsets = find(b"OOFF", "object offsets")?;
    if offsets.len() != names.len() * 8 {
        bail!("multi-pack-index object offset chunk is the wrong size");
    }
    let large = chunks.iter().find(|(chunk, _)| chunk == b"LOFF").map(|(_, data)| *data).unwrap_or_default();
    let mut objects = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let pack = u32::from_be_bytes(offsets[i * 8..i * 8 + 4].try_into()?);
        let raw = u32::from_be_bytes(offsets[i * 8 + 4..i * 8 + 8].try_into()?);
        let offset = if raw & 0x8000_0000 != 0 {
            let at = (raw & 0x7fff_ffff) as usize * 8;
            u64::from_be_bytes(large.get(at..at + 8).ok_or_else(|| anyhow::anyhow!("multi-pack-index large offset out of bounds"))?.try_into()?)
        } else {
            raw as u64
        };
        if pack as usize >= pack_count {
            bail!("bad pack-int-id: {} ({} total packs)", pack, pack_count);
        }
        objects.push((*name, pack, offset));
    }
    Ok((pack_names, objects))
}

/// Checks a multi-pack-index against the packs on disk: structure and checksum, every
/// covered pack present, and every object at the recorded offset in the recorded pack.
/// Returns the problems found (empty = fine).
// Used by `multi-pack-index verify`.
pub fn verify_midx(data: &[u8], packs: &[RepoPack]) -> Vec<String> {
    let (pack_names, objects) = match parse_midx(data) {
        Ok(parsed) => parsed,
        Err(e) => return vec![e.to_string()],
    };
    let mut problems = Vec::new();
    let covered: Vec<Option<&RepoPack>> = pack_names.iter().map(|name| packs.iter().find(|p| format!("{}.idx", p.name) == *name)).collect();
    for (name, pack) in pack_names.iter().zip(&covered) {
        if pack.is_none() {
            problems.push(format!("failed to load pack-index for packfile {}", name.trim_end_matches(".idx")));
        }
    }
    for (name, pack, offset) in &objects {
        let Some(pack) = covered[*pack as usize] else { continue };
        match find_in_index(&pack.index, name) {
            Some(i) if pack.index.offsets[i] == *offset => {}
            Some(i) => problems.push(format!("incorrect object offset for oid[{}] = {}: {:x} != {:x}", i, hex::encode(name), pack.index.offsets[i], offset)),
            None => problems.push(format!("failed to load pack entry for oid = {}", hex::encode(name))),
        }
    }
    problems
}

/// Replaces the multi-pack-index file.
// Used by `multi-pack-index write` / `expire`.
pub fn store_midx(bytes: &[u8]) -> Result<()> {
    let path = midx_path();
    if path.exists() {
        let mut permissions = fs::metadata(&path)?.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions)?;
    }
    fs::write(&path, bytes)?;
    Ok(())
}
