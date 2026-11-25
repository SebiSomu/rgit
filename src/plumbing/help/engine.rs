use clap::CommandFactory;

use crate::cli::Cli;
use crate::plumbing::help::types::CommandHelp;

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


/// Git's "common commands" (`git help`'s short list): typing the start of one of these, such
/// as `rgit stat`, is taken as the best possible match for it.
// Used by `similar_commands`.
const COMMON_COMMANDS: &[&str] = &[
    "add", "bisect", "branch", "clone", "commit", "diff", "fetch", "grep", "init", "log", "merge", "mv", "pull", "push",
    "rebase", "reset", "restore", "rm", "show", "status", "switch", "tag",
];

/// Scores at or above this are too far off to suggest (git's `SIMILARITY_FLOOR`).
// Used by `similar_commands`.
const SIMILARITY_FLOOR: usize = 7;

/// The rgit commands closest to the unknown command `typed`, best first: every command tied for
/// the best score, or none when even the best is not similar enough. A port of git's
/// `help_unknown_cmd`: a prefix of a common command scores 1, anything else 1 + the weighted
/// Damerau-Levenshtein distance (swap 0, substitution 2, insertion 1, deletion 3).
// Used by `rgit <unknown command>` and `help <unknown command>`.
pub fn similar_commands(typed: &str) -> Vec<String> {
    let mut scored: Vec<(usize, String)> = all_command_names()
        .into_iter()
        .map(|candidate| {
            let score = if COMMON_COMMANDS.contains(&candidate.as_str()) && candidate.starts_with(typed) {
                1
            } else {
                levenshtein(typed, &candidate, 0, 2, 1, 3) + 1
            };
            (score, candidate)
        })
        .collect();
    scored.sort();
    let Some(&(best, _)) = scored.first() else { return Vec::new() };
    if best >= SIMILARITY_FLOOR {
        return Vec::new();
    }
    scored.into_iter().take_while(|(score, _)| *score == best).map(|(_, name)| name).collect()
}

/// Weighted Damerau-Levenshtein distance turning `from` into `to`, with costs for swapping two
/// adjacent characters, substituting, inserting and deleting one (git's `levenshtein.c`).
// Used by `similar_commands`.
fn levenshtein(from: &str, to: &str, swap: usize, substitution: usize, insertion: usize, deletion: usize) -> usize {
    let a = from.as_bytes();
    let b = to.as_bytes();
    let mut row0 = vec![0; b.len() + 1];
    let mut row1: Vec<usize> = (0..=b.len()).map(|j| j * insertion).collect();
    let mut row2 = vec![0; b.len() + 1];
    for i in 0..a.len() {
        row2[0] = (i + 1) * deletion;
        for j in 0..b.len() {
            row2[j + 1] = row1[j] + if a[i] != b[j] { substitution } else { 0 };
            if i > 0 && j > 0 && a[i - 1] == b[j] && a[i] == b[j - 1] {
                row2[j + 1] = row2[j + 1].min(row0[j - 1] + swap);
            }
            row2[j + 1] = row2[j + 1].min(row1[j + 1] + deletion).min(row2[j] + insertion);
        }
        std::mem::swap(&mut row0, &mut row1);
        std::mem::swap(&mut row1, &mut row2);
    }
    row1[b.len()]
}
