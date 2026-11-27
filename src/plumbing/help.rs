pub mod catalog;
pub mod cli;
pub mod engine;
pub mod types;

use std::io::Write;
use std::process::Command;
use anyhow::Result;

use crate::plumbing::help::catalog::{description_of, COMMAND_GROUPS, COMMON_GROUPS};
use crate::plumbing::help::engine::{command_help, similar_commands, synonyms};
use crate::plumbing::help::types::CommandHelp;

/// `rgit help [-a | --all] [<command>]`
///
/// Without a command, the commands most work starts with, grouped as in git; with `-a`, every
/// command, grouped as in the user guide, each with a one-line description. With a command
/// name or synonym, prints that command's usage and options: for commands that parse their
/// own arguments this is exactly what `rgit <command> -h` prints, for the others the help
/// clap generates. Works outside a repository. Exit status: 0, 1 for an unknown command,
/// 129 for an unknown option.
pub fn help(args: Vec<String>) -> Result<()> {
    let mut topic: Option<&str> = None;
    for arg in &args {
        match arg.as_str() {
            "-a" | "--all" => {}
            "-h" => {
                println!("usage: rgit help [-a | --all] [<command>]");
                crate::ui::exit(129);
            }
            other if other.starts_with('-') => {
                eprintln!("error: unknown option `{}'\nusage: rgit help [-a | --all] [<command>]", other.trim_start_matches('-'));
                crate::ui::exit(129);
            }
            other => {
                topic.get_or_insert(other);
            }
        }
    }

    let Some(topic) = topic else {
        let all = args.iter().any(|a| a == "-a" || a == "--all");
        show(&if all { all_commands_text() } else { common_commands_text() });
        return Ok(());
    };

    match command_help(topic) {
        Some(CommandHelp::Clap(text)) => show(&text),
        Some(CommandHelp::Raw(name)) => {
            // The command prints its own usage for `-h` (and exits 129); show that as the help.
            let output = Command::new(std::env::current_exe()?).args([name.as_str(), "-h"]).output()?;
            // A few commands answer `-h` with "unknown switch" before their usage; drop that line.
            let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            let text: String = text.split_inclusive('\n').filter(|line| !line.starts_with("error: unknown switch `h'")).collect();
            show(&text);
        }
        None => unknown_command(topic),
    }
    Ok(())
}

/// Reports that `name` is not an rgit command, suggests the most similar ones (like git's
/// "The most similar command is ..."), and exits with status 1.
// Used by `cli::handle_commands` (`rgit <unknown>`) and `help <unknown>`.
pub fn unknown_command(name: &str) -> ! {
    let mut message = format!("rgit: '{}' is not a rgit command. See 'rgit help'.", name);
    let similar = similar_commands(name);
    if !similar.is_empty() {
        message.push_str(if similar.len() == 1 { "\n\nThe most similar command is" } else { "\n\nThe most similar commands are" });
        for command in &similar {
            message.push_str("\n\t");
            message.push_str(command);
        }
    }
    crate::ui::die(1, &message)
}

/// Writes help text to standard output; a closed pipe (`rgit help | head`) is not an error.
fn show(text: &str) {
    let _ = std::io::stdout().lock().write_all(text.as_bytes());
}

/// Column where descriptions start in the command lists (as in git's).
const NAME_WIDTH: usize = 24;

/// `rgit help -a`: every command by group, with its description, then the synonyms.
// Used by `help -a`.
pub fn all_commands_text() -> String {
    let mut text = String::from("See 'rgit help <command>' to read about a specific command\n");
    for (group, commands) in COMMAND_GROUPS {
        text.push_str(&format!("\n{}\n", group));
        for (name, description) in commands.iter() {
            text.push_str(&format!("   {:<width$}{}\n", name, description, width = NAME_WIDTH));
        }
    }
    let synonyms = synonyms();
    if !synonyms.is_empty() {
        let shown: Vec<String> = synonyms.iter().map(|(alias, command)| format!("{} = {}", alias, command)).collect();
        text.push_str(&format!("\nSynonyms: {}\n", shown.join(", ")));
    }
    text.push_str("\nCommands marked (rgit) exist only in rgit.\n");
    text
}

/// `rgit help`: the usage line and the common commands by what they're for, as git prints them.
// Used by `help` without arguments.
pub fn common_commands_text() -> String {
    let mut text = String::from("usage: rgit [--explain | --teach | --plain] <command> [<args>]\n\nThese are common rgit commands used in various situations:\n");
    let width = COMMON_GROUPS.iter().flat_map(|(_, c)| c.iter()).map(|c| c.len()).max().unwrap_or(0) + 3;
    for (group, commands) in COMMON_GROUPS {
        text.push_str(&format!("\n{}\n", group));
        for name in commands.iter() {
            let description = description_of(name).unwrap_or_default().trim_end_matches(" (rgit)");
            text.push_str(&format!("   {:<width$}{}\n", name, description, width = width));
        }
    }
    text.push_str("\n'rgit help -a' lists every command, 'rgit help <command>' shows the options of one.\n");
    text
}
