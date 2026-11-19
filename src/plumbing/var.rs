pub mod cli;
pub mod engine;

use anyhow::Result;

use crate::core::config::read_all_config_entries;
use crate::plumbing::var::engine::{var_value, VAR_NAMES, VAR_USAGE};

/// `rgit var (-l | <variable>)`
///
/// Prints the value of one of rgit's logical variables, or with `-l` every configuration
/// entry (`key=value`) followed by all of the variables (`NAME=value`):
/// `GIT_AUTHOR_IDENT` / `GIT_COMMITTER_IDENT` (the identity and timestamp a commit made now
/// would get), `GIT_EDITOR` and `GIT_SEQUENCE_EDITOR` (`RGIT_EDITOR` / `RGIT_SEQUENCE_EDITOR`,
/// `core.editor`, `VISUAL`, `EDITOR`), `GIT_PAGER` (`RGIT_PAGER`, `core.pager`, `PAGER`,
/// `less`), `GIT_DEFAULT_BRANCH` (`init.defaultBranch`, else `main`), `GIT_SHELL_PATH` and
/// `GIT_CONFIG_GLOBAL` (the global config file). The names are git's, so scripts written for
/// `git var` work. Exit status: 0; 1 when a known variable has no value; an unknown name, a
/// missing argument or extra arguments print the usage and exit 129, like git.
pub fn var(args: Vec<String>) -> Result<()> {
    let usage = || -> ! {
        eprint!("{}", VAR_USAGE);
        crate::ui::exit(129);
    };
    match args.as_slice() {
        [flag] if flag == "-l" => {
            for (key, value) in read_all_config_entries() {
                println!("{}={}", key, value);
            }
            for name in VAR_NAMES {
                if let Some(value) = var_value(name)? {
                    println!("{}={}", name, value);
                }
            }
        }
        [name] => match var_value(name)? {
            Some(value) => println!("{}", value),
            None if VAR_NAMES.contains(&name.as_str()) => crate::ui::exit(1),
            None => usage(),
        },
        _ => usage(),
    }
    Ok(())
}
