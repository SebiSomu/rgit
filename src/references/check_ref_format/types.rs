/// Options for the `check-ref-format` command, which validates a proposed ref name against
/// git's ref naming rules (and, with `--branch`, against the rules for a branch name).
// Used by: src/cli.rs, src/references/check_ref_format.rs
// Used by command/helper: the `check_ref_format` command and `check_refname_format`
#[derive(Debug, Clone, Default)]
pub struct CheckRefFormatOptions {
    /// `--normalize` / `--print`: strip leading slashes, collapse repeated ones, and print the result if valid.
    pub normalize: bool,
    /// `--allow-onelevel`: accept names with a single component (e.g. `main`); off by default.
    pub allow_onelevel: bool,
    /// `--refspec-pattern`: allow a single `*` anywhere in the name, as in a refspec.
    pub refspec_pattern: bool,
    /// `--branch <name>`: check `<name>` as a branch name and print it if valid.
    pub branch: Option<String>,
    /// The ref name to check (not used with `--branch`).
    pub refname: Option<String>,
}
