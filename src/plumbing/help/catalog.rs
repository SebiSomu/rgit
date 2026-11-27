//! The list `rgit help -a` prints: every command, in the groups the user guide uses, with a
//! one-line description. A unit test checks that it names exactly the commands `cli.rs`
//! defines, so a new command can't be left out.

/// The groups of commands, in order, each with its commands and their one-line descriptions.
// Used by `help -a` and the catalog unit test.
pub const COMMAND_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Getting started and configuration",
        &[
            ("init", "Create an empty repository or reinitialize an existing one"),
            ("clone", "Copy a repository into a new directory"),
            ("config", "Get and set repository or global options"),
            ("var", "Show a logical variable (identity, editor, pager, ...)"),
            ("version", "Show the rgit version"),
            ("help", "Show the commands, or the options of one"),
        ],
    ),
    (
        "Everyday work: stage, commit, clean up",
        &[
            ("status", "Show the working tree status"),
            ("add", "Add file contents to the index"),
            ("rm", "Remove files from the working tree and the index"),
            ("mv", "Move or rename a file or a directory"),
            ("commit", "Record changes to the repository"),
            ("restore", "Restore working tree files or unstage them"),
            ("clean", "Remove untracked files from the working tree"),
            ("ignore", "Ignore files and stop tracking them (rgit)"),
        ],
    ),
    (
        "History and inspection",
        &[
            ("log", "Show commit logs"),
            ("show", "Show commits, tags, trees and blobs"),
            ("diff", "Show changes between commits, the index and the working tree"),
            ("blame", "Show which commit last changed each line of a file"),
            ("annotate", "Annotate file lines with commit information"),
            ("whatchanged", "Show logs with the files each commit changed"),
            ("shortlog", "Summarize the log by author"),
            ("grep", "Print lines matching a pattern"),
            ("describe", "Name a commit after the nearest tag"),
            ("name-rev", "Name commits relative to refs"),
            ("show-branch", "Show branches and their commits side by side"),
            ("last-modified", "Show the commit that last changed each path"),
            ("range-diff", "Compare two versions of a series of commits"),
            ("cherry", "Find commits not yet applied upstream"),
            ("repo", "Show information about the repository"),
        ],
    ),
    (
        "Branches, tags and working trees",
        &[
            ("branch", "List, create, rename or delete branches"),
            ("switch", "Switch branches"),
            ("checkout", "Switch branches or restore working tree files"),
            ("tag", "Create, list or delete tags"),
            ("worktree", "Manage several working trees of one repository"),
            ("sparse-checkout", "Check out only part of the working tree"),
            ("submodule", "Manage repositories inside the repository"),
        ],
    ),
    (
        "Merging, rebasing and replaying",
        &[
            ("merge", "Join two or more histories together"),
            ("conflicts", "List unresolved conflicts with who changed what (rgit)"),
            ("merge-base", "Find the best common ancestors of commits"),
            ("rebase", "Reapply commits on top of another base"),
            ("cherry-pick", "Apply the changes of existing commits"),
            ("revert", "Undo the changes of existing commits with new commits"),
            ("replay", "Replay commits onto a new base without touching the work tree"),
            ("rerere", "Reuse recorded resolutions of conflicts"),
            ("banana-split", "Split a commit into one commit per path (rgit)"),
            ("merge-tree", "Merge trees without touching the index or work tree"),
            ("merge-file", "Run a three-way merge on one file"),
        ],
    ),
    (
        "Undo and recovery",
        &[
            ("undo", "Take back the last operation, even reset --hard (rgit)"),
            ("redo", "Bring back what undo took back (rgit)"),
            ("oops", "After a mistake, the most likely fixes, ready to run (rgit)"),
            ("recover", "Find lost work and bring it back (rgit)"),
            ("wip", "Save unfinished work on a hidden per-branch ref (rgit)"),
            ("squash", "Fold the last N commits into one (rgit)"),
            ("reword", "Change the message of HEAD or an earlier commit (rgit)"),
            ("drop", "Remove one commit and replay the ones after it (rgit)"),
            ("fixup", "Fold staged changes into an earlier commit (rgit)"),
            ("absorb", "Fold each staged hunk into the commit that last touched it (rgit)"),
            ("split", "Split a commit into several by hunk (rgit)"),
            ("move-commit", "Move commits to another branch without switching (rgit)"),
            ("sync", "Fetch, then fast-forward, rebase or merge onto the upstream (rgit)"),
            ("reset", "Move HEAD, and optionally the index and work tree"),
            ("stash", "Set changes aside in a dirty working tree"),
            ("reflog", "Show and manage where refs have been"),
            ("bisect", "Find the commit that introduced a bug by binary search"),
        ],
    ),
    (
        "Understanding and learning",
        &[
            ("whereami", "Where you are and what to run next (rgit)"),
            ("story", "The history of a file or directory in plain words (rgit)"),
            ("hotspots", "Files that change most, with most authors, together (rgit)"),
            ("stats", "Numbers on the branch's history (rgit)"),
            ("timeline", "A day-by-day digest of the repository (rgit)"),
            ("compare", "Where two branches stand against each other (rgit)"),
            ("contributors", "Who wrote the commits, with shares and dates (rgit)"),
            ("tip", "A git tip of the day (rgit)"),
            ("zen", "The ideas rgit is built on (rgit)"),
        ],
    ),
    (
        "Safety and health",
        &[
            ("secrets", "Look for keys, tokens and passwords before you share (rgit)"),
            ("scrub", "Remove a file or folder from all history, with a backup (rgit)"),
            ("doctor", "Check the repository for problems, fix the safe ones (rgit)"),
            ("backup", "Save every ref to a dated bundle (rgit)"),
            ("fsck", "Verify the objects and their connectivity"),
            ("gc", "Clean up and optimize the repository"),
            ("prune", "Remove unreachable objects"),
        ],
    ),
    (
        "Review and release",
        &[
            ("pr-summary", "A Markdown pull request description (rgit)"),
            ("open", "Open the web page of a file, branch or commit (rgit)"),
            ("changelog", "A Markdown changelog from conventional commits (rgit)"),
        ],
    ),
    (
        "Remotes and collaboration",
        &[
            ("remote", "Manage the remotes this repository knows"),
            ("fetch", "Download objects and refs from another repository"),
            ("push", "Update remote refs and send the objects they need"),
            ("pull", "Fetch and integrate with another repository or branch"),
            ("ls-remote", "List the refs of another repository"),
            ("credential", "Fill, approve or reject logins through the credential helpers"),
            ("credential-store", "Keep logins in ~/.git-credentials"),
            ("bundle", "Move objects and refs in a single file"),
            ("request-pull", "Summarize changes for a pull request"),
            ("fmt-merge-msg", "Build a merge commit message"),
        ],
    ),
    (
        "Patches and email",
        &[
            ("apply", "Apply a patch to files and/or the index"),
            ("format-patch", "Prepare patches for email"),
            ("am", "Apply a series of patches from a mailbox"),
            ("mailsplit", "Split a mailbox into one file per message"),
            ("mailinfo", "Extract the patch and authorship from one message"),
            ("interpret-trailers", "Add or parse trailers in commit messages"),
            ("check-mailmap", "Show canonical names and emails from .mailmap"),
            ("patch-id", "Compute a unique id for a patch"),
        ],
    ),
    (
        "Export and import",
        &[
            ("archive", "Create a tar or zip archive of a tree"),
            ("get-tar-commit-id", "Extract the commit id from an archive"),
            ("fast-export", "Export history as a fast-import stream"),
            ("fast-import", "Import history from a fast-import stream"),
        ],
    ),
    (
        "Objects, refs and the index (plumbing)",
        &[
            ("hash-object", "Compute an object id and optionally store the object"),
            ("cat-file", "Show the content, type or size of objects"),
            ("write-tree", "Create a tree object from the index"),
            ("ls-tree", "List the contents of a tree object"),
            ("commit-tree", "Create a new commit object"),
            ("mktree", "Build a tree object from ls-tree style text"),
            ("mktag", "Create a tag object, with extra validation"),
            ("unpack-file", "Write a blob's content to a temporary file"),
            ("rev-parse", "Resolve revisions and query the repository"),
            ("rev-list", "List commit objects in reverse chronological order"),
            ("ls-files", "Show information about files in the index and work tree"),
            ("check-ignore", "Debug gitignore and exclude files"),
            ("check-ref-format", "Check that a ref name is well formed"),
            ("show-ref", "List refs in the repository"),
            ("symbolic-ref", "Read, change or delete symbolic refs"),
            ("update-ref", "Update the object a ref points at, safely"),
            ("pack-refs", "Pack refs into packed-refs"),
            ("for-each-ref", "Output information on each ref"),
            ("refs", "Low-level access to refs"),
            ("replace", "Create, list or delete refs that replace objects"),
            ("notes", "Add or inspect notes on objects"),
            ("update-index", "Register file contents in the working tree to the index"),
            ("checkout-index", "Copy files from the index to the working tree"),
            ("read-tree", "Read tree information into the index"),
            ("diff-tree", "Compare the content and mode of blobs in two trees"),
            ("diff-index", "Compare a tree to the working tree or index"),
            ("diff-files", "Compare files in the working tree and the index"),
            ("stripspace", "Remove unnecessary whitespace"),
            ("column", "Display data in columns"),
        ],
    ),
    (
        "Storage and maintenance",
        &[
            ("repack", "Pack unpacked objects in a repository"),
            ("pack-objects", "Create a packed archive of objects"),
            ("unpack-objects", "Unpack objects from a packed archive"),
            ("index-pack", "Build the index file for a pack"),
            ("verify-pack", "Validate packed archive files"),
            ("show-index", "Show a pack's index file"),
            ("prune-packed", "Remove loose objects that are already in packs"),
            ("commit-graph", "Write and verify commit-graph files"),
            ("multi-pack-index", "Write and verify multi-pack-indexes"),
            ("count-objects", "Count unpacked objects and their disk use"),
            ("maintenance", "Run tasks that keep the repository fast"),
            ("for-each-repo", "Run an rgit command in a list of repositories"),
            ("hook", "Run hooks"),
        ],
    ),
];

/// The shorter list `rgit help` prints, as git's does: the commands most work starts with.
// Used by `help` without arguments.
pub const COMMON_GROUPS: &[(&str, &[&str])] = &[
    ("start a working area", &["clone", "init"]),
    ("work on the current change", &["add", "mv", "restore", "rm"]),
    ("examine the history and state", &["bisect", "diff", "grep", "log", "show", "status", "whereami"]),
    ("grow, mark and tweak your common history", &["branch", "commit", "merge", "rebase", "reset", "switch", "tag"]),
    ("collaborate", &["fetch", "pull", "push"]),
    ("when something went wrong (rgit)", &["undo", "oops", "recover"]),
];

/// The one-line description of `name` in the catalog.
// Used by `help` without arguments.
pub fn description_of(name: &str) -> Option<&'static str> {
    COMMAND_GROUPS.iter().flat_map(|(_, commands)| commands.iter()).find(|(n, _)| *n == name).map(|(_, d)| *d)
}

#[cfg(test)]
#[path = "../../../tests/unit/plumbing/help_catalog.rs"]
mod tests;
