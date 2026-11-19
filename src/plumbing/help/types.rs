/// How `help <command>` shows a command: `Raw` commands parse their own arguments and print
/// their own usage for `-h` (the value is the command name); `Clap` holds rendered clap help.
// Used by: src/plumbing/help.rs, src/plumbing/help/engine.rs
// Used by command/helper: `command_help` and the `help` command
#[derive(Debug, Clone)]
pub enum CommandHelp {
    Raw(String),
    Clap(String),
}
