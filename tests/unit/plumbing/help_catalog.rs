//! Unit tests for `src/plumbing/help/catalog.rs`, compiled into that module via `#[path]`.

use super::*;
use clap::CommandFactory;

#[test]
fn the_catalog_lists_every_command_exactly_once() {
    let mut defined: Vec<String> = crate::cli::Cli::command().get_subcommands().map(|s| s.get_name().to_string()).collect();
    defined.sort();
    let mut listed: Vec<String> = COMMAND_GROUPS.iter().flat_map(|(_, c)| c.iter()).map(|(n, _)| n.to_string()).collect();
    listed.sort();
    let mut unique = listed.clone();
    unique.dedup();
    assert_eq!(listed, unique, "a command is listed twice in the catalog");
    let missing: Vec<&String> = defined.iter().filter(|d| !listed.contains(d)).collect();
    let unknown: Vec<&String> = listed.iter().filter(|l| !defined.contains(l)).collect();
    assert!(missing.is_empty() && unknown.is_empty(), "add to src/plumbing/help/catalog.rs: {:?}; not commands: {:?}", missing, unknown);
}

#[test]
fn descriptions_fit_on_one_terminal_line() {
    for (_, commands) in COMMAND_GROUPS {
        for (name, description) in commands.iter() {
            assert!(3 + 24 + description.len() <= 100, "{}'s description is too long for an 100-column line", name);
            assert!(!description.ends_with('.'), "{}'s description ends with a period (git's don't)", name);
        }
    }
}

#[test]
fn common_commands_are_in_the_catalog() {
    for (_, commands) in COMMON_GROUPS {
        for name in commands.iter() {
            assert!(description_of(name).is_some(), "{} is in the common list but not in the catalog", name);
        }
    }
}
