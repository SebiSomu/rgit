use clap::CommandFactory;

use crate::cli::Cli;
use crate::objects::CommandHelp;

/// Names of every rgit command, including synonyms such as `stage`, sorted alphabetically.
// Used by `help` and `help -a`.
pub fn all_command_names() -> Vec<String> {
    let root = Cli::command();
    let mut names: Vec<String> = Vec::new();
    for sub in root.get_subcommands() {
        names.push(sub.get_name().to_string());
        names.extend(sub.get_all_aliases().map(str::to_string));
    }
    names.sort();
    names.dedup();
    names
}

/// Looks a command up by name or synonym and says how its help is produced; `None` when rgit
/// has no such command.
// Used by `help <command>`.
pub fn command_help(name: &str) -> Option<CommandHelp> {
    let mut root = Cli::command().bin_name("rgit");
    root.build();
    let sub = root.find_subcommand_mut(name)?;
    let canonical = sub.get_name().to_string();
    let parses_own_args = sub.is_disable_help_flag_set() && sub.get_arguments().any(|a| a.get_id() == "args");
    if parses_own_args {
        Some(CommandHelp::Raw(canonical))
    } else {
        Some(CommandHelp::Clap(sub.render_long_help().to_string()))
    }
}
