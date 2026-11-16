use anyhow::{bail, Result};
use sha1::{Digest, Sha1};

/// Assembles a git "chunk format" file (used by the commit-graph and the multi-pack-index):
/// the given header, a table of contents with one `(4-byte id, 8-byte offset)` row per chunk
/// and a terminating row holding the end offset, the chunks themselves, and a trailing
/// SHA-1 of everything before it.
// Used by `write_commit_graph` and `write_midx`.
pub fn assemble_chunk_file(header: &[u8], chunks: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = header.to_vec();
    let mut offset = (header.len() + (chunks.len() + 1) * 12) as u64;
    for (id, data) in chunks {
        out.extend_from_slice(id);
        out.extend_from_slice(&offset.to_be_bytes());
        offset += data.len() as u64;
    }
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&offset.to_be_bytes());
    for (_, data) in chunks {
        out.extend_from_slice(data);
    }
    let checksum: [u8; 20] = Sha1::digest(&out).into();
    out.extend_from_slice(&checksum);
    out
}

/// Reads the table of contents of a chunk-format file whose header is `header_len` bytes and
/// announces `count` chunks, after checking the trailing checksum. Returns `(id, data)` for
/// each chunk.
// Used by `verify_commit_graph` and `verify_midx`.
pub fn parse_chunk_file(data: &[u8], header_len: usize, count: usize) -> Result<Vec<([u8; 4], &[u8])>> {
    if data.len() < header_len + (count + 1) * 12 + 20 {
        bail!("file is too small to hold its table of contents");
    }
    let body_len = data.len() - 20;
    if Sha1::digest(&data[..body_len]).as_slice() != &data[body_len..] {
        bail!("the checksum at the end of the file does not match its contents");
    }
    let row = |i: usize| -> ([u8; 4], usize) {
        let at = header_len + i * 12;
        let mut id = [0u8; 4];
        id.copy_from_slice(&data[at..at + 4]);
        let mut offset = [0u8; 8];
        offset.copy_from_slice(&data[at + 4..at + 12]);
        (id, u64::from_be_bytes(offset) as usize)
    };
    let mut chunks = Vec::new();
    for i in 0..count {
        let (id, start) = row(i);
        let (_, end) = row(i + 1);
        if start > end || end > body_len {
            bail!("chunk {} has an offset outside the file", String::from_utf8_lossy(&id));
        }
        chunks.push((id, &data[start..end]));
    }
    Ok(chunks)
}

/// The 256-entry fan-out table for sorted object names: entry `n` counts the names whose
/// first byte is `<= n`.
// Used by `write_commit_graph` and `write_midx`.
pub fn fanout_table(sorted: &[[u8; 20]]) -> Vec<u8> {
    let mut counts = [0u32; 256];
    for hash in sorted {
        counts[hash[0] as usize] += 1;
    }
    let mut out = Vec::with_capacity(1024);
    let mut running = 0u32;
    for count in counts {
        running += count;
        out.extend_from_slice(&running.to_be_bytes());
    }
    out
}

/// Checks an `OIDF` / `OIDL` chunk pair: sizes, fan-out consistency and strictly ascending
/// names. Returns the names, or what is wrong.
// Used by `verify_commit_graph` and `verify_midx`.
pub fn read_oid_chunks(fanout: &[u8], lookup: &[u8]) -> Result<Vec<[u8; 20]>> {
    if fanout.len() != 1024 {
        bail!("the OID fanout chunk has the wrong size");
    }
    if lookup.len() % 20 != 0 {
        bail!("the OID lookup chunk has the wrong size");
    }
    let names: Vec<[u8; 20]> = lookup.chunks(20).map(|c| c.try_into().unwrap_or([0; 20])).collect();
    for pair in names.windows(2) {
        if pair[0] >= pair[1] {
            bail!("the OID lookup is out of order: {} then {}", hex::encode(pair[0]), hex::encode(pair[1]));
        }
    }
    if fanout_table(&names) != fanout {
        bail!("the OID fanout does not match the OID lookup");
    }
    Ok(names)
}
