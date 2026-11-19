/// One file pair in the diff `log --follow` shows for a commit: the old and new path with their
/// tree entries, and the rename/copy status and similarity (`R`/`C`, git's 0..60000 score) when
/// the pair was found by rename detection.
// Used by: src/history/follow.rs
// Used by command/helper: `follow_commit_pairs`, `render_follow_diff`, and the `log` command
#[derive(Debug, Clone)]
pub struct FollowPair {
    pub old_path: String,
    pub new_path: String,
    pub old: Option<([u8; 20], u32)>,
    pub new: Option<([u8; 20], u32)>,
    /// `Some(('R' | 'C', score))` for a detected rename or copy.
    pub rename: Option<(char, u32)>,
}
