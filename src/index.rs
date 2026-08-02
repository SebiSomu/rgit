use anyhow::{Context, Result};
use sha1::{Digest, Sha1};
use std::fs;
use std::path::Path;

const INDEX_PATH: &str = ".git/index";
const SIGNATURE: &[u8; 4] = b"DIRC";
const VERSION: u32 = 2;

#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub ctime_secs: u32,
    pub ctime_nsecs: u32,
    pub mtime_secs: u32,
    pub mtime_nsecs: u32,
    pub dev: u32,
    pub ino: u32,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u32,
    pub hash: [u8; 20],
    pub path: String,
}

pub fn read_index() -> Result<Vec<IndexEntry>> {
    if !Path::new(INDEX_PATH).exists() {
        return Ok(Vec::new());
    }

    let data = fs::read(INDEX_PATH).context("Failed to read .git/index")?;

    if data.len() < 12 {
        anyhow::bail!("Corrupt index: file too short");
    }
    if &data[0..4] != SIGNATURE {
        anyhow::bail!("Corrupt index: bad signature");
    }

    let version = u32::from_be_bytes(data[4..8].try_into()?);
    if version != VERSION {
        anyhow::bail!("Unsupported index version: {}", version);
    }

    let entry_count = u32::from_be_bytes(data[8..12].try_into()?) as usize;

    let mut entries = Vec::with_capacity(entry_count);
    let mut pos = 12;

    for _ in 0..entry_count {
        let entry_start = pos;

        let ctime_secs = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;
        let ctime_nsecs = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;
        let mtime_secs = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;
        let mtime_nsecs = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;
        let dev = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;
        let ino = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;
        let mode = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;
        let uid = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;
        let gid = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;
        let size = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;

        let mut hash = [0u8; 20];
        hash.copy_from_slice(&data[pos..pos + 20]);
        pos += 20;

        let flags = u16::from_be_bytes(data[pos..pos + 2].try_into()?);
        pos += 2;
        if flags & 0x4000 != 0 {
            pos += 2;
        }

        let name_len = (flags & 0x0FFF) as usize;
        let name_end = pos + name_len;
        let path = String::from_utf8_lossy(&data[pos..name_end]).to_string();
        pos = name_end;
        let entry_len = pos - entry_start;
        let padding = 8 - (entry_len % 8);
        pos += padding;

        entries.push(IndexEntry {
            ctime_secs,
            ctime_nsecs,
            mtime_secs,
            mtime_nsecs,
            dev,
            ino,
            mode,
            uid,
            gid,
            size,
            hash,
            path,
        });
    }

    Ok(entries)
}

pub fn write_index(entries: &mut Vec<IndexEntry>) -> Result<()> {
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let mut buf = Vec::new();
    buf.extend_from_slice(SIGNATURE);
    buf.extend_from_slice(&VERSION.to_be_bytes());
    buf.extend_from_slice(&(entries.len() as u32).to_be_bytes());

    for entry in entries.iter() {
        let entry_start = buf.len();

        buf.extend_from_slice(&entry.ctime_secs.to_be_bytes());
        buf.extend_from_slice(&entry.ctime_nsecs.to_be_bytes());
        buf.extend_from_slice(&entry.mtime_secs.to_be_bytes());
        buf.extend_from_slice(&entry.mtime_nsecs.to_be_bytes());
        buf.extend_from_slice(&entry.dev.to_be_bytes());
        buf.extend_from_slice(&entry.ino.to_be_bytes());
        buf.extend_from_slice(&entry.mode.to_be_bytes());
        buf.extend_from_slice(&entry.uid.to_be_bytes());
        buf.extend_from_slice(&entry.gid.to_be_bytes());
        buf.extend_from_slice(&entry.size.to_be_bytes());
        buf.extend_from_slice(&entry.hash);

        let name_bytes = entry.path.as_bytes();
        let name_len = name_bytes.len().min(0x0FFF) as u16;
        buf.extend_from_slice(&name_len.to_be_bytes());
        buf.extend_from_slice(name_bytes);

        let entry_len = buf.len() - entry_start;
        let padding = 8 - (entry_len % 8);
        buf.extend(std::iter::repeat(0u8).take(padding));
    }

    let mut hasher = Sha1::new();
    hasher.update(&buf);
    buf.extend_from_slice(&hasher.finalize());

    fs::write(INDEX_PATH, &buf).context("Failed to write .git/index")?;

    Ok(())
}

struct StatInfo {
    ctime_secs: u32,
    ctime_nsecs: u32,
    mtime_secs: u32,
    mtime_nsecs: u32,
    dev: u32,
    ino: u32,
    uid: u32,
    gid: u32,
    executable: bool,
}

#[cfg(unix)]
fn stat_info(metadata: &fs::Metadata) -> StatInfo {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    StatInfo {
        ctime_secs: metadata.ctime() as u32,
        ctime_nsecs: metadata.ctime_nsec() as u32,
        mtime_secs: metadata.mtime() as u32,
        mtime_nsecs: metadata.mtime_nsec() as u32,
        dev: metadata.dev() as u32,
        ino: metadata.ino() as u32,
        uid: metadata.uid(),
        gid: metadata.gid(),
        executable: metadata.permissions().mode() & 0o111 != 0,
    }
}

#[cfg(not(unix))]
fn stat_info(metadata: &fs::Metadata) -> StatInfo {
    use std::time::UNIX_EPOCH;

    let to_secs_nsecs = |t: std::io::Result<std::time::SystemTime>| -> (u32, u32) {
        t.ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|d| (d.as_secs() as u32, d.subsec_nanos()))
            .unwrap_or((0, 0))
    };

    let (mtime_secs, mtime_nsecs) = to_secs_nsecs(metadata.modified());
    let (ctime_secs, ctime_nsecs) = {
        let (s, n) = to_secs_nsecs(metadata.created());
        if s == 0 && n == 0 {
            (mtime_secs, mtime_nsecs)
        } else {
            (s, n)
        }
    };

    StatInfo {
        ctime_secs,
        ctime_nsecs,
        mtime_secs,
        mtime_nsecs,
        dev: 0,
        ino: 0,
        uid: 0,
        gid: 0,
        executable: false,
    }
}

pub fn build_entry(rel_path: &str, hash: [u8; 20], metadata: &fs::Metadata) -> IndexEntry {
    let stat = stat_info(metadata);
    let mode: u32 = if stat.executable { 0o100755 } else { 0o100644 };

    IndexEntry {
        ctime_secs: stat.ctime_secs,
        ctime_nsecs: stat.ctime_nsecs,
        mtime_secs: stat.mtime_secs,
        mtime_nsecs: stat.mtime_nsecs,
        dev: stat.dev,
        ino: stat.ino,
        mode,
        uid: stat.uid,
        gid: stat.gid,
        size: metadata.len() as u32,
        hash,
        path: rel_path.to_string(),
    }
}