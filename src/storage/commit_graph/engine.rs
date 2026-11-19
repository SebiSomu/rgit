use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::storage::chunk_file::{assemble_chunk_file, fanout_table, parse_chunk_file, read_oid_chunks};
use crate::core::object_store::read_object;

/// "No parent" marker in the commit data chunk.
const NO_PARENT: u32 = 0x7000_0000;
/// Flag: the second-parent field is an index into the extra edge list / the last edge.
const EXTRA_EDGES: u32 = 0x8000_0000;

/// Path of the commit-graph file.
// Used by `commit-graph write` / `verify`.
pub fn commit_graph_path() -> PathBuf {
    crate::core::repo::path(".git/objects/info/commit-graph")
}

/// The fields of a commit the graph stores: tree, parents and committer time. `None` when
/// the object is missing or not a commit.
// Used by `write_commit_graph` and `verify_commit_graph`.
fn commit_fields(hash: &str) -> Option<([u8; 20], Vec<String>, u64)> {
    let (kind, content) = read_object(hash).ok()?;
    if kind != "commit" {
        return None;
    }
    let text = String::from_utf8_lossy(&content);
    let mut tree = [0u8; 20];
    let mut parents = Vec::new();
    let mut date = 0u64;
    for line in text.lines().take_while(|l| !l.is_empty()) {
        if let Some(value) = line.strip_prefix("tree ") {
            hex::decode_to_slice(value.trim(), &mut tree).ok()?;
        } else if let Some(value) = line.strip_prefix("parent ") {
            parents.push(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("committer ") {
            date = value.rsplit_once('>').and_then(|(_, rest)| rest.split_whitespace().next()).and_then(|t| t.parse().ok()).unwrap_or(0);
        }
    }
    Some((tree, parents, date))
}

/// Builds a commit-graph file (version 1, SHA-1) for the given commits and all their
/// ancestors: chunks `OIDF` (fan-out), `OIDL` (sorted names), `CDAT` (tree, two parent
/// positions, topological level and commit time), `GDA2` (corrected commit date offsets,
/// with `GDO2` for offsets over 31 bits) and `EDGE` (third and later parents of octopus
/// merges). Returns the file bytes and the number of commits, or `None` when there are none.
// Used by `commit-graph write`.
pub fn write_commit_graph(starts: &[String]) -> Result<Option<(Vec<u8>, usize)>> {
    // Close the set under parents.
    let mut commits: BTreeMap<String, ([u8; 20], Vec<String>, u64)> = BTreeMap::new();
    let mut stack: Vec<String> = starts.to_vec();
    while let Some(hash) = stack.pop() {
        if commits.contains_key(&hash) {
            continue;
        }
        let Some(fields) = commit_fields(&hash) else { continue };
        stack.extend(fields.1.iter().cloned());
        commits.insert(hash, fields);
    }
    if commits.is_empty() {
        return Ok(None);
    }
    let position: HashMap<&str, u32> = commits.keys().enumerate().map(|(i, hash)| (hash.as_str(), i as u32)).collect();

    // Topological level (1 for roots) and corrected commit date, parents before children.
    let mut level: HashMap<&str, (u32, u64)> = HashMap::new();
    for start in commits.keys() {
        let mut stack: Vec<&str> = vec![start.as_str()];
        while let Some(&hash) = stack.last() {
            if level.contains_key(hash) {
                stack.pop();
                continue;
            }
            let (_, parents, date) = &commits[hash];
            let missing: Vec<&str> = parents.iter().map(String::as_str).filter(|p| commits.contains_key(*p) && !level.contains_key(p)).collect();
            if !missing.is_empty() {
                stack.extend(missing);
                continue;
            }
            let known: Vec<(u32, u64)> = parents.iter().filter_map(|p| level.get(p.as_str()).copied()).collect();
            let generation = known.iter().map(|(g, _)| *g).max().unwrap_or(0) + 1;
            let corrected = known.iter().map(|(_, c)| *c + 1).max().unwrap_or(0).max(*date);
            level.insert(hash, (generation, corrected));
            stack.pop();
        }
    }

    let names: Vec<[u8; 20]> = commits.keys().map(|hash| {
        let mut bytes = [0u8; 20];
        let _ = hex::decode_to_slice(hash, &mut bytes);
        bytes
    }).collect();
    let (mut data, mut dates, mut overflow, mut edges) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (hash, (tree, parents, date)) in &commits {
        let parent_position = |i: usize| parents.get(i).and_then(|p| position.get(p.as_str()).copied()).unwrap_or(NO_PARENT);
        data.extend_from_slice(tree);
        data.extend_from_slice(&parent_position(0).to_be_bytes());
        if parents.len() > 2 {
            data.extend_from_slice(&(EXTRA_EDGES | (edges.len() / 4) as u32).to_be_bytes());
            for i in 1..parents.len() {
                let last = if i == parents.len() - 1 { EXTRA_EDGES } else { 0 };
                edges.extend_from_slice(&(parent_position(i) | last).to_be_bytes());
            }
        } else {
            data.extend_from_slice(&parent_position(1).to_be_bytes());
        }
        let (generation, corrected) = level[hash.as_str()];
        data.extend_from_slice(&((generation.min(0x3fff_ffff) << 2) | ((*date >> 32) & 3) as u32).to_be_bytes());
        data.extend_from_slice(&(*date as u32).to_be_bytes());
        let offset = corrected - date;
        if offset > 0x7fff_ffff {
            dates.extend_from_slice(&(EXTRA_EDGES | (overflow.len() / 8) as u32).to_be_bytes());
            overflow.extend_from_slice(&offset.to_be_bytes());
        } else {
            dates.extend_from_slice(&(offset as u32).to_be_bytes());
        }
    }

    let mut chunks: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"OIDF", fanout_table(&names)),
        (*b"OIDL", names.iter().flatten().copied().collect()),
        (*b"CDAT", data),
        (*b"GDA2", dates),
    ];
    if !overflow.is_empty() {
        chunks.push((*b"GDO2", overflow));
    }
    if !edges.is_empty() {
        chunks.push((*b"EDGE", edges));
    }
    let header = [b'C', b'G', b'P', b'H', 1, 1, chunks.len() as u8, 0];
    Ok(Some((assemble_chunk_file(&header, &chunks), commits.len())))
}

/// The commit names stored in an existing commit-graph file (empty when there is none or it
/// cannot be read).
// Used by `commit-graph write --append`.
pub fn commit_graph_commits() -> Vec<String> {
    let Ok(data) = fs::read(commit_graph_path()) else { return Vec::new() };
    if data.len() < 8 || &data[..4] != b"CGPH" {
        return Vec::new();
    }
    let Ok(chunks) = parse_chunk_file(&data, 8, data[6] as usize) else { return Vec::new() };
    chunks.iter().find(|(id, _)| id == b"OIDL").map(|(_, lookup)| lookup.chunks(20).map(hex::encode).collect()).unwrap_or_default()
}

/// Checks a commit-graph file against the object store: header, checksum, chunk layout,
/// sorted names, and for every commit its tree, parents, commit time and topological
/// level. Returns the problems found (empty = fine).
// Used by `commit-graph verify`.
pub fn verify_commit_graph(data: &[u8]) -> Vec<String> {
    let mut problems = Vec::new();
    let parsed = (|| -> Result<(Vec<[u8; 20]>, Vec<u8>, Vec<u8>)> {
        if data.len() < 8 || &data[..4] != b"CGPH" {
            bail!("commit-graph signature does not match signature 43475048");
        }
        if data[4] != 1 {
            bail!("commit-graph version {} does not match version 1", data[4]);
        }
        if data[5] != 1 {
            bail!("commit-graph hash version {} does not match version 1", data[5]);
        }
        let chunks = parse_chunk_file(data, 8, data[6] as usize)?;
        let find = |id: &[u8; 4]| chunks.iter().find(|(chunk, _)| chunk == id).map(|(_, data)| *data);
        let names = read_oid_chunks(find(b"OIDF").ok_or_else(|| anyhow::anyhow!("commit-graph is missing the OID Fanout chunk"))?, find(b"OIDL").ok_or_else(|| anyhow::anyhow!("commit-graph is missing the OID Lookup chunk"))?)?;
        let commit_data = find(b"CDAT").ok_or_else(|| anyhow::anyhow!("commit-graph is missing the Commit Data chunk"))?;
        if commit_data.len() != names.len() * 36 {
            bail!("commit-graph commit data chunk is wrong size");
        }
        Ok((names, commit_data.to_vec(), find(b"EDGE").unwrap_or_default().to_vec()))
    })();
    let (names, commit_data, edges) = match parsed {
        Ok(parts) => parts,
        Err(e) => return vec![e.to_string()],
    };

    let be32 = |bytes: &[u8], at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap_or([0; 4]));
    let generation_of = |position: u32| be32(&commit_data, position as usize * 36 + 28) >> 2;
    for (i, name) in names.iter().enumerate() {
        let hash = hex::encode(name);
        let row = &commit_data[i * 36..(i + 1) * 36];
        let Some((tree, parents, date)) = commit_fields(&hash) else {
            problems.push(format!("failed to parse commit {} from object database for commit-graph", hash));
            continue;
        };
        if row[..20] != tree {
            problems.push(format!("root tree OID for commit {} in commit-graph is {} != {}", hash, hex::encode(&row[..20]), hex::encode(tree)));
        }
        // Parent positions as stored.
        let mut stored: Vec<u32> = Vec::new();
        let (first, second) = (be32(row, 20), be32(row, 24));
        if first != NO_PARENT {
            stored.push(first);
        }
        if second & EXTRA_EDGES != 0 {
            let mut at = (second & !EXTRA_EDGES) as usize * 4;
            loop {
                if at + 4 > edges.len() {
                    problems.push(format!("commit-graph parent list for commit {} is too long", hash));
                    break;
                }
                let edge = be32(&edges, at);
                stored.push(edge & !EXTRA_EDGES);
                if edge & EXTRA_EDGES != 0 {
                    break;
                }
                at += 4;
            }
        } else if second != NO_PARENT {
            stored.push(second);
        }
        let stored_names: Vec<String> = stored.iter().map(|p| names.get(*p as usize).map(hex::encode).unwrap_or_else(|| "(out of range)".to_string())).collect();
        if stored_names != parents {
            if stored_names.len() != parents.len() {
                problems.push(format!("commit-graph parent list for commit {} {}", hash, if stored_names.len() > parents.len() { "is too long" } else { "terminates early" }));
            } else {
                problems.push(format!("commit-graph parent for {} is {} != {}", hash, stored_names.join(" "), parents.join(" ")));
            }
            continue;
        }
        let stored_date = (((be32(row, 28) & 3) as u64) << 32) | be32(row, 32) as u64;
        if stored_date != date {
            problems.push(format!("commit date for commit {} in commit-graph is {} != {}", hash, stored_date, date));
        }
        let generation = be32(row, 28) >> 2;
        let expected = stored.iter().map(|p| generation_of(*p)).max().unwrap_or(0) + 1;
        if generation != 0 && generation != expected {
            problems.push(format!("commit-graph generation for commit {} is {} != {}", hash, generation, expected));
        }
    }
    problems
}
