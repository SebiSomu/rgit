/// One line of a file's blame result: the line itself and the commit that last changed it,
/// with that commit's author and author time. Lines that exist only in the working copy
/// carry an all-zero commit hash and the author "Not Committed Yet".
// Used by: src/history/blame/types.rs, src/history/blame/engine.rs
// Used by command/helper: `blame_file`, `format_blame_output`, and the `blame` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub commit: String,
    pub author: String,
    pub timestamp: u64,
    pub tz: String,
    pub line_no: usize,
    pub content: String,
    pub boundary: bool,
}

impl BlameLine {
    pub fn hash_label(&self) -> String {
        if self.boundary {
            format!("^{}", self.commit.chars().take(7).collect::<String>())
        } else {
            self.commit.chars().take(8).collect()
        }
    }
}
