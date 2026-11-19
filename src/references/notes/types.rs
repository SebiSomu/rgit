/// A loaded notes ref: its name, the notes commit it points at (if any), the notes as
/// `annotated object -> note blob`, and whatever else its tree contains.
// Used by: src/references/notes.rs, src/references/notes/engine.rs
// Used by command/helper: `load_notes_tree`, `commit_notes_tree`, `merge_notes`, and the `notes` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotesTree {
    pub notes_ref: String,
    pub commit: Option<String>,
    pub notes: std::collections::BTreeMap<String, String>,
    pub other: std::collections::BTreeMap<String, ([u8; 20], u32)>,
}

/// How `notes merge` settles a note that both sides changed differently (`-s <strategy>`).
// Used by: src/references/notes.rs, src/references/notes/engine.rs
// Used by command/helper: `merge_notes` and the `notes` command
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NotesMergeStrategy {
    /// Stop and report the conflict (git's default; rgit cannot continue from there).
    #[default]
    Manual,
    /// Keep the local note.
    Ours,
    /// Take the remote note.
    Theirs,
    /// Both notes, separated by a blank line.
    Union,
    /// The unique lines of both notes, sorted.
    CatSortUniq,
}
