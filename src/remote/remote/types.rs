use clap::Subcommand;

/// Subcommands supported by the `remote` command for managing tracking repositories.
// Used by: src/cli.rs, src/remote/remote.rs
// Used by command/helper: CLI argument parser and the `remote` command dispatcher
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum RemoteAction {
    Add {
        name: String,
        url: String,
    },
    Remove {
        name: String,
    },
    Rm {
        name: String,
    },
    Rename {
        old_name: String,
        new_name: String,
    },
    GetUrl {
        #[arg(long = "push")]
        push: bool,
        name: String,
    },
    SetUrl {
        name: String,
        new_url: String,
    },
    Show {
        name: String,
    },
}

/// Detailed information about a configured remote repository.
// Used by: src/remote/remote/engine.rs
// Used by command/helper: `get_remote_info` helper and the `remote show` command
#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub fetch_url: Option<String>,
    pub push_url: Option<String>,
    pub tracking_branches: Vec<String>,
}
