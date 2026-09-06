use clap::Subcommand;

#[derive(Debug)]
pub enum GitObject {
    Blob(Vec<u8>),
    Tree(Vec<TreeEntry>),
    Commit(CommitData),
}

#[derive(Debug)]
pub struct TreeEntry {
    pub mode: String,
    pub name: String,
    pub hash: [u8; 20],
}

#[derive(Debug)]
pub struct CommitData {
    pub tree_hash: String,
    pub parent_hashes: Vec<String>,
    pub author: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitIgnoreRule {
    pub pattern: String,
    pub base_dir: String,
    pub negated: bool,
    pub dir_only: bool,
    pub has_slash: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffOp {
    Keep(String),
    Delete(String),
    Insert(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeConflict {
    pub path: String,
    pub base: Vec<u8>,
    pub ours: Vec<u8>,
    pub theirs: Vec<u8>,
}

#[derive(Subcommand, Debug)]
pub enum StashAction {
    Push {
        #[arg(short = 'm', long = "message")]
        message: Option<String>,
    },
    Pop {
        stash: Option<String>,
    },
    Apply {
        stash: Option<String>,
    },
    List,
    Drop {
        stash: Option<String>,
    },
    Show {
        stash: Option<String>,
    },
    Clear,
}

pub struct StashEntry {
    pub hash: String,
    pub message: String,
}

pub enum BisectOutcome {
    WaitingForBad,
    WaitingForGood,
    Continue(String, usize),
    Found(String),
}

#[derive(Subcommand, Debug)]
pub enum BisectAction {
    Start {
        bad: Option<String>,
        good: Vec<String>,
    },
    Bad {
        rev: Option<String>,
    },
    Good {
        rev: Option<String>,
    },
    Skip {
        revs: Vec<String>,
    },
    Reset {
        commit: Option<String>,
    },
    Log,
    Run {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}