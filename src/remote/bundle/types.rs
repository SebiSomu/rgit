use clap::Subcommand;

/// Subcommands of `bundle`, which packs commits and the objects they need into a single file
/// for offline transfer, and inspects or unpacks such files.
// Used by: src/cli.rs, src/remote/bundle.rs
// Used by command/helper: CLI argument parser and the `bundle` command dispatcher
#[derive(Subcommand, Debug)]
pub enum BundleAction {
    /// Write a bundle with the commits selected by <rev-list-args> (e.g. `main`, `--all`, `v1..main`)
    Create {
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        /// Accepted for compatibility; progress is only shown on a terminal
        #[arg(long = "progress")]
        progress: bool,
        /// Bundle format to write: 2 (default) or 3
        #[arg(long = "version", value_name = "VERSION")]
        version: Option<u8>,
        /// The bundle file to write (`-` for standard output)
        file: String,
        #[arg(required = true, allow_hyphen_values = true, num_args = 1.., value_name = "REV-LIST-ARGS")]
        revs: Vec<String>,
    },
    /// Check that a bundle is valid and can be applied to this repository
    Verify {
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        file: String,
    },
    /// List the refs a bundle contains (only the named ones, if any are given)
    ListHeads {
        file: String,
        refnames: Vec<String>,
    },
    /// Store a bundle's objects in this repository and print its refs
    Unbundle {
        /// Accepted for compatibility; progress is only shown on a terminal
        #[arg(long = "progress")]
        progress: bool,
        file: String,
        refnames: Vec<String>,
    },
}

/// The parsed header of a bundle file: its format version, `@key=value` capabilities (v3),
/// the prerequisite commits a repository must already have (with their comments), the refs
/// it carries, and where the packfile starts.
// Used by: src/remote/bundle.rs, src/remote/bundle/engine.rs
// Used by command/helper: `parse_bundle_header`, `check_bundle_prerequisites`, and the `bundle` command
#[derive(Debug, Clone, Default)]
pub struct BundleHeader {
    pub version: u8,
    pub capabilities: Vec<(String, Option<String>)>,
    pub prerequisites: Vec<(String, String)>,
    pub refs: Vec<(String, String)>,
    pub pack_offset: usize,
}
