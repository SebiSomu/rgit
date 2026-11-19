/// An instruction action assigned to an individual commit entry in an interactive rebase todo file.
// Used by: src/merge/rebase/types.rs, src/merge/rebase.rs, src/merge/rebase/engine.rs
// Used by command/helper: `parse_edited_todo`, `render_todo_for_editing`, `commit_item`, and the `rebase` command
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
}

impl Action {
    pub fn parse(word: &str) -> Option<Action> {
        match word {
            "pick" | "p" => Some(Action::Pick),
            "reword" | "r" => Some(Action::Reword),
            "edit" | "e" => Some(Action::Edit),
            "squash" | "s" => Some(Action::Squash),
            "fixup" | "f" => Some(Action::Fixup),
            "drop" | "d" => Some(Action::Drop),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Action::Pick => "pick",
            Action::Reword => "reword",
            Action::Edit => "edit",
            Action::Squash => "squash",
            Action::Fixup => "fixup",
            Action::Drop => "drop",
        }
    }
}

/// A parsed line entry from `.git/REBASE_TODO` tracking a commit, its instruction action, and subject.
// Used by: src/merge/rebase/types.rs, src/merge/rebase.rs, src/merge/rebase/engine.rs
// Used by command/helper: `read_todo`, `write_todo`, `render_todo_for_editing`, `process_rebase_todo`, and the `rebase` command
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoItem {
    pub action: Action,
    pub hash: String,
    pub subject: String,
}

impl TodoItem {
    // Unused before the refactor too (hidden while it lived in the `pub` objects.rs).
    #[allow(dead_code)]
    pub fn format_line(&self) -> String {
        let short_hash = &self.hash[..self.hash.len().min(7)];
        format!("{} {} {}", self.action.name(), short_hash, self.subject)
    }
}

/// Execution outcome of applying a single todo item during the rebase replay cycle.
// Used by: src/merge/rebase/engine.rs
// Used by command/helper: `commit_item`, `rebase_continue`, and `process_rebase_todo` helpers
#[derive(Debug, PartialEq, Eq)]
pub enum ItemResult {
    Done,
    PausedForEdit,
}

/// Parsed arguments of `rgit rebase`: what to replay (`<upstream>..<branch>`), where to
/// (`--onto`, `--keep-base`), and the mode (`-i`, `--continue`, `--abort`, `--skip`,
/// `--edit-todo`).
// Used by: src/cli.rs, src/merge/rebase.rs
// Used by command/helper: the `rebase` command
#[derive(Debug, Clone, Default)]
pub struct RebaseOptions {
    /// `<upstream>`; the branch's upstream when omitted.
    pub upstream: Option<String>,
    /// `<branch>`: switched to before rebasing.
    pub branch: Option<String>,
    /// `--onto <newbase>`: replay onto this instead of `<upstream>`.
    pub onto: Option<String>,
    /// `--keep-base`: replay onto the merge base of `<upstream>` and the branch.
    pub keep_base: bool,
    /// `--[no-]reapply-cherry-picks`; unset means "no" except with `--keep-base`.
    pub reapply_cherry_picks: Option<bool>,
    pub interactive: bool,
    pub cont: bool,
    pub abort: bool,
    pub skip: bool,
    pub edit_todo: bool,
}
