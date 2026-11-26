//! Unit tests for `src/workspace/squash/engine.rs`, compiled into that module via `#[path]`.
//! The command itself is covered by the `squash` scenario in `tests/cli/scenarios/squash.rs`.

use super::*;
use crate::core::types::ParsedCommit;

fn plan(messages: &[&str]) -> SquashPlan {
    let commits = messages.iter().map(|m| ParsedCommit { message: format!("{}\n", m), ..Default::default() }).collect();
    SquashPlan { commits, base: None }
}

#[test]
fn without_the_editor_the_messages_are_joined_oldest_first() {
    let p = plan(&["Add parser\n\nHandles numbers.", "fixup! Add parser", "Parse strings too"]);
    assert_eq!(combined_message(&p, false), "Add parser\n\nHandles numbers.\n\nParse strings too\n");
}

#[test]
fn the_first_message_stays_even_when_it_is_a_fixup() {
    let p = plan(&["fixup! Old work", "fixup! Old work"]);
    assert_eq!(combined_message(&p, false), "fixup! Old work\n");
}

#[test]
fn the_editor_gets_git_s_squash_template() {
    let p = plan(&["Add parser", "fixup! Add parser\n\nwith a body", "Parse strings"]);
    let expected = "# This is a combination of 3 commits.\n\
                    # This is the 1st commit message:\n\nAdd parser\n\n\
                    # The commit message #2 will be skipped:\n\n# fixup! Add parser\n#\n# with a body\n\n\
                    # This is the commit message #3:\n\nParse strings\n";
    assert_eq!(combined_message(&p, true), expected);
    // What git's cleanup keeps from it: the two real messages.
    assert_eq!(crate::core::commit::cleanup_commit_message(&combined_message(&p, true), true), "Add parser\n\nParse strings\n");
}
