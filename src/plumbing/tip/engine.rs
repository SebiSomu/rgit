//! The tips `rgit tip` shows (git concepts and rgit's own commands), the one of the day, and
//! the text of `rgit zen`.

use crate::plumbing::tip::types::Tip;

/// Every tip, in a fixed order (their numbers).
pub const TIPS: &[Tip] = &[
    Tip { title: "Undo almost anything", text: "`rgit undo` takes back the last command that changed the repository: a commit, reset, rebase, merge or deleted branch. `rgit redo` brings it back.", example: "rgit undo --list" },
    Tip { title: "Ask what went wrong", text: "Just did something you regret? `rgit oops` looks at what happened (a commit on the wrong branch, a deleted branch, a detached HEAD) and suggests the commands that fix it.", example: "rgit oops" },
    Tip { title: "One screen on where you are", text: "`rgit whereami` shows the branch, its upstream, the last commit, your changes, any operation in progress, and the command you most likely want next.", example: "rgit whereami" },
    Tip { title: "Three trees", text: "Git keeps three versions of every file: the last commit (HEAD), the staging area (the index), and the working tree. `add` copies to the index, `commit` records the index.", example: "rgit status --short" },
    Tip { title: "Stage part of a file", text: "`rgit add -p` shows each change in a file and asks whether to stage it, so one file can go into two commits.", example: "rgit add -p" },
    Tip { title: "Unstage without losing anything", text: "`rgit restore --staged <file>` takes a file out of the staging area; your edits stay in the file.", example: "rgit restore --staged notes.txt" },
    Tip { title: "Uncommit", text: "`rgit reset --soft HEAD~1` takes the last commit off the branch and leaves its changes staged, ready to commit again (`rgit commit -C ORIG_HEAD` keeps the message).", example: "rgit reset --soft HEAD~1" },
    Tip { title: "Fix an older commit", text: "Stage the fix and run `rgit fixup`: pick the commit from a list and the fix is folded into it, rewriting the commits on top.", example: "rgit fixup" },
    Tip { title: "Let rgit find the commit", text: "`rgit absorb` puts each staged hunk into the commit that last changed those lines, all in one rewrite.", example: "rgit absorb -n" },
    Tip { title: "Split a commit", text: "`rgit split <commit>` walks you through its hunks and turns one commit into several; the commits on top keep their content.", example: "rgit split HEAD" },
    Tip { title: "Committed on the wrong branch?", text: "`rgit move-commit <commit> <branch>` copies a commit to another branch and drops it here, without switching branches. `-c` creates the branch.", example: "rgit move-commit HEAD topic -c" },
    Tip { title: "Save work in progress", text: "`rgit wip` saves your changes as a commit on a hidden ref and leaves a clean tree; `rgit wip --restore` brings them back.", example: "rgit wip \"halfway through the parser\"" },
    Tip { title: "Stash includes untracked files only if asked", text: "`rgit stash` saves changes to tracked files; new files stay behind unless you pass `-u`.", example: "rgit stash push -u -m \"before the rebase\"" },
    Tip { title: "Go back to the previous branch", text: "`rgit switch -` returns to the branch you were on before the last switch, like `cd -`.", example: "rgit switch -" },
    Tip { title: "Relative revisions", text: "`HEAD~2` is two commits before HEAD (following first parents); `HEAD^2` is a merge's second parent; `main@{1}` is where main was before its last move.", example: "rgit log --oneline -3 HEAD~2" },
    Tip { title: "Two dots and three dots", text: "`a..b` means the commits b has that a doesn't; `a...b` means the commits either side has that the other doesn't. `diff a...b` compares b with where they split.", example: "rgit log --oneline main..feature" },
    Tip { title: "Compare two branches", text: "`rgit compare <a> <b>` says whether a is ahead, behind or diverged, lists the commits on each side, the files that differ, and whether a merge would conflict.", example: "rgit compare main" },
    Tip { title: "Stay in sync", text: "`rgit sync` fetches, then fast-forwards or rebases (or merges) onto the upstream, stashing around it, and `-p` pushes after.", example: "rgit sync -n" },
    Tip { title: "Rebase versus merge", text: "A merge records that two lines of work came together; a rebase replays your commits on top of the other line, as if you had started there. Only rebase commits nobody else has.", example: "rgit rebase -i HEAD~3" },
    Tip { title: "Conflicts explained", text: "When a merge or rebase stops, `rgit conflicts` lists the files, where the conflicts are and who changed each side; `--explain <file>` shows them side by side.", example: "rgit conflicts" },
    Tip { title: "The reflog remembers", text: "Every move of HEAD is logged for weeks. A commit you lost after a reset or rebase is still in `rgit reflog`, and a branch can be made at it.", example: "rgit reflog" },
    Tip { title: "Find the commit that broke it", text: "`rgit bisect` halves the history until it finds the first bad commit; `bisect run <script>` does it unattended.", example: "rgit bisect start HEAD v1.0" },
    Tip { title: "Who changed this line?", text: "`rgit blame <file>` shows the commit that last changed each line, following renames; `-L 10,20` limits it to some lines.", example: "rgit blame -L 1,5 README.md" },
    Tip { title: "A file's whole story", text: "`rgit story <file>` tells when it was created and by whom, its renames, big rewrites, who wrote it today, and a timeline of changes.", example: "rgit story src/main.rs" },
    Tip { title: "Where the churn is", text: "`rgit hotspots` lists the files that change most, have the most authors, and change together: good places to look for trouble.", example: "rgit hotspots -n 5" },
    Tip { title: "Ignore a file that is already tracked", text: "A .gitignore rule never untracks a file by itself. `rgit ignore <pattern>` adds the rule and stops tracking the files it matches, keeping them on disk.", example: "rgit ignore 'build/'" },
    Tip { title: "Keep secrets out", text: "`rgit secrets` scans the staged changes for keys, tokens and passwords; as a pre-commit hook (`exec rgit secrets`) it stops the commit.", example: "rgit secrets --history" },
    Tip { title: "Check the repository's health", text: "`rgit doctor` finds stale lock files, broken worktrees, missing identity, unreachable config and huge untracked files, with the fix for each; `--fix` applies the safe ones.", example: "rgit doctor --quick" },
    Tip { title: "Back it all up", text: "`rgit backup` writes every ref to a dated bundle in ~/.rgit-backups and keeps the newest ten; `rgit clone <file> <dir>` restores one.", example: "rgit backup --list" },
    Tip { title: "Several checkouts of one repository", text: "`rgit worktree add <dir> <branch>` checks a branch out in another directory, sharing the same history: no second clone, no stashing to switch.", example: "rgit worktree add ../hotfix main" },
    Tip { title: "Copy one commit", text: "`rgit cherry-pick <commit>` applies a commit's change on top of the current branch; `-x` notes where it came from.", example: "rgit cherry-pick -x abc1234" },
    Tip { title: "Annotated tags carry a message", text: "`rgit tag -a v1.0 -m \"First release\"` makes a tag object with its own message, author and date; `rgit describe` names a commit after the nearest tag.", example: "rgit describe --tags" },
    Tip { title: "Write the release notes from history", text: "With conventional commit messages (`feat:`, `fix:`), `rgit changelog` groups the commits since the last tag into Markdown.", example: "rgit changelog --version v1.1.0" },
    Tip { title: "Draft the pull request", text: "`rgit pr-summary` prints a Markdown description of your branch (title, commits, files, and whether it merges cleanly) and `rgit open pr` opens the page to create it.", example: "rgit pr-summary" },
    Tip { title: "Make rgit explain more", text: "`rgit --explain <command>` adds a line on what each change means; `--teach` also shows the refs and index changes underneath; `--plain` prints exactly what git would.", example: "rgit --explain status" },
    Tip { title: "Clean, but look first", text: "`rgit clean -n` lists the untracked files `clean -f` would delete. Deleted untracked files can't be undone.", example: "rgit clean -n" },
];

/// The tip of the day: the same one all day (UTC), a different one tomorrow.
// Used by the `tip` command.
pub fn tip_of_the_day() -> usize {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0);
    (days as usize) % TIPS.len()
}

/// Splits `text` into lines of at most `width` characters, at spaces.
// Used by the `tip` command.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The Zen of rgit: the ideas the project is built on.
pub const ZEN: &str = "The Zen of rgit

Explain first, act second.
Say what happened, not what ran.
Every change can be undone; every undo, redone.
A refusal is better than a surprise.
Check everything, then touch anything: nothing half-done.
Uncommitted work is sacred.
Speak git's formats; speak plainly about them.
The next step belongs in the output.
Shared history is shared: rewrite only what is yours.
One command, one whole job.
If it needs a tutorial, it needs a better message.
";
