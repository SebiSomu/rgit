use std::path::PathBuf;

/// Represents an uncompressed Git database object holding its decoded payload.
// Used by: src/core/types.rs
// Used by command/helper: Object serialization and decoding helpers
#[derive(Debug)]
pub enum GitObject {
    Blob(Vec<u8>),
    Tree(Vec<TreeEntry>),
    Commit(CommitData),
}

/// Represents a single file or directory record stored within a parsed Git tree object.
// Used by: src/core/types.rs
// Used by command/helper: Tree parsing and traversal helpers
#[derive(Debug)]
pub struct TreeEntry {
    pub mode: String,
    pub name: String,
    pub hash: [u8; 20],
}

/// Represents the deserialized header fields and message body of a commit object.
// Used by: src/core/types.rs
// Used by command/helper: Commit parsing and resolution helpers
#[derive(Debug)]
pub struct CommitData {
    pub tree_hash: String,
    pub parent_hashes: Vec<String>,
    pub author: String,
    pub message: String,
}

/// An in-memory flattened map of relative file paths to their 20-byte object hash and mode.
// Used by: src/core/types.rs, src/merge/banana_split.rs
// Used by command/helper: `flatten_tree`, `resolve_tree_from_source`, `build_tree_from_map`, and the `banana_split` command
pub type Tree = std::collections::BTreeMap<String, ([u8; 20], u32)>;

/// One line of a reflog: a ref moving from one commit to another, who did it, when, and why.
/// Stored one per line in `.git/logs/<ref>` in git's own format:
/// `<old> <new> <name> <<email>> <timestamp> <tz>\t<message>`.
// Used by: src/core/types.rs, src/core/reflog.rs
// Used by command/helper: `append_reflog_entry`, `read_reflog`, `write_reflog`, `expire_reflog`, `format_reflog_output`, and the `reflog` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry {
    /// Hash the ref pointed at before (all zeros if it was just created).
    pub old_hash: String,
    /// Hash the ref points at after (all zeros if it was deleted).
    pub new_hash: String,
    /// `Name <email>` of whoever made the move.
    pub identity: String,
    /// When the move happened, as seconds since the Unix epoch.
    pub timestamp: u64,
    /// Timezone offset of `timestamp`, as `+HHMM` / `-HHMM`.
    pub tz: String,
    /// What caused the move, e.g. `commit -m fix typo` or `switch feature`.
    pub message: String,
}

impl ReflogEntry {
    /// Renders the entry as one line of a reflog file (without the trailing newline). An
    /// empty message is written without the tab, as git does.
    pub fn to_line(&self) -> String {
        let head = format!("{} {} {} {} {}", self.old_hash, self.new_hash, self.identity, self.timestamp, self.tz);
        if self.message.is_empty() { head } else { format!("{}\t{}", head, self.message) }
    }

    /// Parses one line of a reflog file; `None` if it is malformed.
    pub fn parse(line: &str) -> Option<ReflogEntry> {
        let (head, message) = line.split_once('\t').unwrap_or((line, ""));
        let old_hash = head.get(0..40)?;
        let new_hash = head.get(41..81)?;
        let is_hash = |h: &str| h.chars().all(|c| c.is_ascii_hexdigit());
        if !is_hash(old_hash) || !is_hash(new_hash) || head.as_bytes().get(40) != Some(&b' ') {
            return None;
        }

        let rest = head.get(82..)?;
        let close = rest.rfind('>')?;
        let identity = rest[..=close].trim().to_string();
        let mut tail = rest[close + 1..].split_whitespace();
        let timestamp: u64 = tail.next()?.parse().ok()?;
        let tz = tail.next().unwrap_or("+0000").to_string();

        Some(ReflogEntry {
            old_hash: old_hash.to_string(),
            new_hash: new_hash.to_string(),
            identity,
            timestamp,
            tz,
            message: message.to_string(),
        })
    }
}

/// A loose object file found in `.git/objects/xx/`: what it is called, where it lives, how
/// large the compressed file is and when it was last modified (which is how `gc` decides an
/// unreferenced object is old enough to prune).
// Used by: src/core/object_store.rs, src/storage/fsck/engine.rs, src/storage/gc/engine.rs, src/storage/gc/types.rs, src/storage/prune.rs
// Used by command/helper: `list_loose_objects`, `plan_gc`, `remove_loose_object`, and the `gc` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LooseObject {
    /// Full 40-digit hash of the object.
    pub hash: String,
    /// Path of the object file inside `.git/objects`.
    pub path: PathBuf,
    /// Size of the compressed object file, in bytes.
    pub size: u64,
    /// Last-modified time of the file, as seconds since the Unix epoch.
    pub modified: u64,
}

/// An author or committer identity from a commit object: name, email, Unix timestamp, and
/// the `+HHMM` timezone it was recorded in.
// Used by: src/core/types.rs, src/core/commit.rs, src/history/log/engine.rs, src/history/rev_list/engine.rs, src/mail/am/engine.rs, src/mail/am/types.rs, src/merge/sequencer.rs, src/workspace/commit_summary.rs
// Used by command/helper: `parse_commit_object`, `format_rfc2822_date`, `parse_mail`, `build_commit_with_author`, and the `format_patch`/`am` commands
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitSignature {
    pub name: String,
    pub email: String,
    pub timestamp: i64,
    pub tz: String,
}

/// A commit object parsed into its parts: tree, parents, author, committer, and raw message.
// Used by: src/core/commit.rs, src/diff/hunks.rs, src/export/fast_export/engine.rs, src/history/cherry/engine.rs, src/history/follow.rs, src/history/log/engine.rs, src/history/rev_list/engine.rs, src/history/rev_list/types.rs, src/history/show_branch/engine.rs, src/history/types.rs, src/mail/format_patch/engine.rs, src/merge/rebase/engine.rs
// Used by command/helper: `parse_commit_object`, `select_patch_commits`, and the `format_patch` command
#[derive(Debug, Clone, Default)]
pub struct ParsedCommit {
    pub hash: String,
    pub tree: String,
    pub parents: Vec<String>,
    pub author: CommitSignature,
    pub committer: CommitSignature,
    pub message: String,
}

/// A parsed pack index (`.idx`, version 1 or 2): the object names it lists in sorted order
/// with each object's offset in the pack and, for version 2, the CRC-32 of its packed bytes.
// Used by: src/core/types.rs, src/core/pack.rs
// Used by command/helper: `parse_pack_index`, `repo_packs`, `read_packed_object`, and the `show_index` / `verify_pack` / `index_pack` / `multi_pack_index` commands
#[derive(Debug, Clone, Default)]
pub struct PackIndexData {
    /// Index format version (1 or 2).
    pub version: u32,
    /// Object names, sorted.
    pub hashes: Vec<[u8; 20]>,
    /// CRC-32 of each object's entry in the pack (empty for version 1).
    pub crcs: Vec<u32>,
    /// Offset of each object's entry in the pack.
    pub offsets: Vec<u64>,
    /// The pack's trailing checksum, as recorded in the index.
    pub pack_checksum: [u8; 20],
}

/// One pack of the repository: its files, its index and (once needed) its bytes.
// Used by: src/core/pack.rs, src/storage/multi_pack_index.rs, src/storage/multi_pack_index/engine.rs
// Used by command/helper: `repo_packs`, `read_packed_object`, `packed_object_names`, and the `repack` / `prune_packed` / `multi_pack_index` / `commit_graph` commands
#[derive(Debug)]
pub struct RepoPack {
    /// File stem, e.g. `pack-1234...`.
    pub name: String,
    /// Path of the `.pack` file.
    pub pack_path: PathBuf,
    /// The parsed `.idx`.
    pub index: PackIndexData,
    /// The pack file's bytes, read on first use.
    pub data: std::sync::OnceLock<Vec<u8>>,
}

/// One entry of a pack as it is stored: where it is, what its header says, what it is a
/// delta against (if anything), and its inflated data (object content, or delta instructions).
// Used by: src/core/pack.rs
// Used by command/helper: `scan_pack`, `resolve_pack_entries`, and the `index_pack` / `verify_pack` / `unpack_objects` commands
#[derive(Debug, Clone)]
pub struct PackEntry {
    /// Offset of the entry in the pack.
    pub offset: u64,
    /// Offset just past the entry.
    pub end: u64,
    /// Type code from the header: 1 commit, 2 tree, 3 blob, 4 tag, 6 offset delta, 7 ref delta.
    pub type_code: u8,
    /// Size from the header: the object's size, or for a delta the size of the delta data.
    pub size: u64,
    /// For an offset delta, the offset of its base entry.
    pub base_offset: Option<u64>,
    /// For a ref delta, the name of its base object.
    pub base_hash: Option<String>,
    /// The inflated data.
    pub data: Vec<u8>,
}

/// A pack entry after delta resolution: the object it stands for.
// Used by: src/core/pack.rs, src/storage/verify_pack.rs
// Used by command/helper: `resolve_pack_entries`, `write_pack_index`, and the `index_pack` / `verify_pack` / `unpack_objects` commands
#[derive(Debug, Clone)]
pub struct PackedObject {
    /// Object name (hex).
    pub hash: String,
    /// `commit`, `tree`, `blob` or `tag`.
    pub kind: String,
    /// The object's content.
    pub content: Vec<u8>,
    /// Offset of its entry in the pack.
    pub offset: u64,
    /// Offset just past its entry.
    pub end: u64,
    /// Size from the entry header (delta size for deltas).
    pub header_size: u64,
    /// CRC-32 of the entry's bytes in the pack.
    pub crc: u32,
    /// Number of deltas between this object and a whole one (0 = stored whole).
    pub depth: u32,
    /// For a delta, the name of the object it is based on.
    pub base: Option<String>,
}

/// Settings for building a pack.
// Used by: src/core/pack.rs, src/storage/pack_objects.rs, src/storage/repack.rs
// Used by command/helper: `build_pack`, and the `pack_objects` / `repack` commands
#[derive(Debug, Clone)]
pub struct PackBuildOptions {
    /// How many earlier objects each object is tried against as a delta base (0 = no deltas).
    pub window: usize,
    /// Longest allowed delta chain.
    pub depth: u32,
}

/// A pack that was just built: its bytes and, per object, name, offset and CRC-32.
// Used by: src/core/pack.rs
// Used by command/helper: `build_pack`, `store_pack`, and the `pack_objects` / `repack` commands
#[derive(Debug, Clone, Default)]
pub struct BuiltPack {
    /// The complete pack, trailer included.
    pub bytes: Vec<u8>,
    /// `(object name, offset, crc32)` for every object, in pack order.
    pub entries: Vec<([u8; 20], u64, u32)>,
    /// How many objects were stored as deltas.
    pub deltas: usize,
}
