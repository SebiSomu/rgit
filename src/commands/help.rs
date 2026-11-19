use std::io::Write;
use std::process::Command;
use anyhow::Result;

use crate::helpers::column::{default_column_options, render_columns};
use crate::helpers::help::{all_command_names, command_help};
use crate::objects::{ColumnEnable, CommandHelp};

/// `rgit help [-a | --all] [<command>]`
///
/// Without a command (or with `-a`), lists every rgit command in columns. With a command
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
        let mut opts = default_column_options();
        opts.enable = ColumnEnable::Always;
        opts.indent = "   ".to_string();
        opts.padding = 2;
        let text = format!(
            "usage: rgit <command> [<args>]\n\nAvailable commands:\n{}\nSee 'rgit help <command>' for the options of a command.\n",
            render_columns(&all_command_names(), &opts)
        );
        show(&text);
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
        None => {
            eprintln!("rgit: '{}' is not a rgit command. See 'rgit help'.", topic);
            crate::ui::exit(1);
        }
    }
    Ok(())
}

/// Writes help text to standard output; a closed pipe (`rgit help | head`) is not an error.
fn show(text: &str) {
    let _ = std::io::stdout().lock().write_all(text.as_bytes());
}
