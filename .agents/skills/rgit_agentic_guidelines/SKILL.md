---
name: rgit_agentic_guidelines
description: Guidelines for agentic coding in the RGit codebase, covering branch testing isolation, commit rules, and architectural patterns.
---

# RGit Agentic Coding Guidelines & Project Overview

This document defines the constraints, protocols, and architectural patterns that must be adhered to by any AI agent working on the `rgit` codebase.

---

## 1. Development & Testing Policies

> [!IMPORTANT]
> **No Direct Commits/Pushes**
> Agents must **never** run `git commit` or `git push` directly in the project repository. The user must review, stage, commit, and push all modifications manually.

> [!WARNING]
> **Strict Test Isolation**
> All execution and testing of the compiled `rgit` binary must be isolated to the `test-sandbox/` directory.
>
> - **Always** use `cargo run --bin rtest -- [args]` to run test commands.
> - **Never** run `cargo run -- init` or other repository-lifecycle commands directly in the repository root (`c:\rgit-main`), as this will overwrite the main project's actual `.git` configuration and head.

---

## 2. Project Architecture & Complete Structure

`rgit` is a modular, lightweight Git implementation in Rust. Its architecture strictly separates command entry points, domain objects, and operational helpers:

```text
src/
├── main.rs                   # Entry point calling cli::handle_commands()
├── cli.rs                    # Parses CLI arguments with clap and routes to commands
├── commands.rs               # Root module declaring and re-exporting subcommands
├── commands/                 # High-level CLI command entry points
│   ├── plumbing.rs           # init, hash-object, cat-file, write-tree, ls-tree
│   ├── commit.rs             # commit, commit-tree, log
│   ├── staging.rs            # add, status, rm, mv
│   ├── branch.rs             # branch, switch, checkout
│   ├── restore.rs            # restore (worktree and staged paths)
│   ├── diff.rs               # diff
│   ├── merge.rs              # merge, merge-base
│   ├── reset.rs              # reset
│   ├── clean.rs              # clean
│   ├── cherry_pick.rs        # cherry-pick (--continue, --abort)
│   ├── revert.rs             # revert (--continue, --abort)
│   ├── stash.rs              # stash (push, pop, apply, list, drop, show, clear)
│   ├── bisect.rs             # bisect (start, bad, good, skip, reset, log, run)
│   ├── tag.rs                # tag, show, describe
│   ├── config.rs             # config (get, set, unset, list)
│   ├── rebase.rs             # rebase (-i, --continue, --abort, --skip, --edit-todo)
│   ├── banana_split.rs       # banana-split (splits commit by path/dir into atomic commits)
│   ├── push.rs               # push (local transport & GitHub Git Data API: object transfers, ff checks, ref updates)
│   ├── fetch.rs              # fetch (local transport & GitHub Git Data API: download objects, update remote-tracking refs)
│   ├── clone.rs              # clone (local transport & GitHub Git Data API: init + fetch + checkout + upstream setup)
│   ├── pull.rs               # pull (fetch from remote + merge upstream branch into current branch)
│   ├── remote.rs             # remote (list, verbose, add, remove/rm, rename, get-url, set-url, show)
│   ├── blame.rs              # blame (line-by-line commit attribution, -L ranges, uncommitted changes)
│   ├── archive.rs            # archive (tar, tar.gz/tgz, zip archives with path filters and prefix)
│   ├── reflog.rs             # reflog (show, exists, delete, expire)
│   ├── gc.rs                 # gc (reflog expiry + unreachable loose object pruning, --dry-run)
│   ├── fsck.rs               # fsck (object integrity, connectivity, dangling/unreachable reports)
│   ├── grep.rs               # grep (worktree, --cached, and revision search with regex/fixed strings)
│   ├── count_objects.rs      # count-objects (-v, -H)
│   ├── rev_parse.rs          # rev-parse (revision resolution & repository property queries)
│   ├── shortlog.rs           # shortlog (-s, -n, -e, --all)
│   ├── ls_files.rs           # ls-files (-c, -m, -d, -o, -i, --exclude-standard, -s, -t, -z, pathspecs)
│   ├── check_ignore.rs       # check-ignore (-q, -v, -n, --no-index, --stdin, -z)
│   ├── check_ref_format.rs   # check-ref-format (--normalize/--print, --[no-]allow-onelevel, --refspec-pattern, --branch)
│   ├── apply.rs              # apply (--check, --stat, --numstat, --summary, --cached, --index, -R, -v, -p<n>)
│   ├── format_patch.rs       # format-patch (-<n>, --root, --stdout, -o, -n/-N, --start-number, --subject-prefix, -k, --signature)
│   ├── am.rs                 # am (mbox/stdin, -k, --continue, --skip, --abort, --quit, --show-current-patch)
│   ├── rev_list.rs           # rev-list (ranges, --not, ref sets, ordering, limits, filters, paths, output formats)
│   └── worktree.rs           # worktree (add, list, remove, move, lock, unlock, prune)
├── helpers.rs                # Root module declaring and re-exporting helper utilities
├── helpers/                  # Lower-level helpers & engines
│   ├── objects.rs            # Object compression, hashing, and tree writing
│   ├── commit.rs             # Commit building, parsing, parent chains, ancestor walks (is_reachable)
│   ├── checkout.rs           # Working tree sync, switch safety checks, index resets
│   ├── diff.rs               # Myers diff algorithm & unified diff formatting
│   ├── gitignore.rs          # Gitignore parsing and pattern matching
│   ├── merge.rs              # Three-way tree merge & conflict marker formatting
│   ├── state_files.rs        # In-progress operation state files, stash list storage and apply/pop
│   ├── config.rs             # Local & global .gitconfig reader/writer/author identity
│   ├── clean.rs              # Directory walker for untracked/ignored cleanup candidates
│   ├── banana_split.rs       # Path grouping helpers for banana-split
│   ├── rebase.rs             # Interactive rebase engine, todo editor, replay runner
│   ├── push.rs               # Remote repo resolution, object enumeration, loose object copier, ref updates
│   ├── github.rs             # GitHub Git Data API client, blob/tree/commit uploader, remote ref updates
│   ├── fetch.rs              # Local filesystem fetch transport, remote branch walking, object downloading
│   ├── clone.rs              # Source classification, skeleton repo creation, target dir prep/cleanup, worktree checkout
│   ├── remote.rs             # Remote config queries, name validation, ref directory manipulation, tracking branch collection
│   ├── blame.rs              # Line attribution algorithm, diff-mapping, tree lookups, date formatting
│   ├── archive.rs            # Archive packing, tar/tar.gz/zip streams, timestamp preservation, path filters
│   ├── reflog.rs             # Reflog append/parse, <ref>@{n} resolution, entry deletion and expiry
│   ├── gc.rs                 # Reachability roots, unreachable loose object detection, pruning
│   ├── fsck.rs               # Object integrity checks, connectivity walks, dangling object reports
│   ├── grep.rs               # Matcher construction, pathspec filtering, content search
│   ├── count_objects.rs      # Loose/packed object statistics, human-readable sizes
│   ├── rev_parse.rs          # Revision syntax resolution, worktree root discovery, property queries
│   ├── shortlog.rs           # Author grouping of reachable commits
│   ├── ls_files.rs           # Worktree-vs-index state, untracked/ignored walker, ls-files pathspec matching
│   ├── check_ignore.rs       # Source/line-tracked ignore rules, deciding-rule lookup, path normalization
│   ├── check_ref_format.rs   # Git ref name rules, slash normalization, branch shorthand validation
│   ├── apply.rs              # Patch parsing (incl. binary), hunk placement, binary deltas, shared check/write, diffstat
│   ├── format_patch.rs       # Commit selection, mail headers, git-compatible diff/hunks, diffstat, binary literals
│   ├── am.rs                 # Mailbox split, header decoding, author-preserving commits, .git/rebase-apply state
│   ├── rev_list.rs           # Arg parsing, git revision-walk simulation, history simplification, topo sort, filters
│   └── worktree.rs           # Worktree listing/lookup, admin dirs, checkout into a new worktree, prune/move, list formatting
│
├── objects.rs                # Special types, enums, structs, and type aliases
├── refs.rs                   # Reference resolution, branch refs, and HEAD state transitions
├── repo.rs                   # Repository path resolution (`repo::path(".git/...")`), main vs linked worktree
├── index.rs                  # Binary staging index (.git/index) reader and writer
└── bin/
    └── rtest.rs              # Isolated test runner executing rgit within test-sandbox/
```

### Module Responsibilities:

1. **[cli.rs](file:///c:/rgit-main/src/cli.rs)**: Parses and routes all command-line arguments using `clap`. All subcommands are declared here.
2. **[commands/](file:///c:/rgit-main/src/commands)**: Contains *only* high-level command entry points. Functions here orchestrate workflows between `helpers`, `refs`, and `index`, and must return `anyhow::Result<()>`. They do not contain low-level helper logic.
3. **[helpers/](file:///c:/rgit-main/src/helpers)**: Standalone operational functions that read/write compressed objects, execute file I/O, spawn editors, walk commit graphs, manage state files, and resolve merge conflicts.
4. **[objects.rs](file:///c:/rgit-main/src/objects.rs)**: Single source of truth for all domain structures, enums, AST items, and shared type aliases used across commands and helpers.
5. **[refs.rs](file:///c:/rgit-main/src/refs.rs)**: Manages reference resolution (`HEAD`, branch heads under `refs/heads/`). Handles attached/detached HEAD state transitions.
6. **[index.rs](file:///c:/rgit-main/src/index.rs)**: Reads and writes the staging index (`.git/index`).
7. **[repo.rs](file:///c:/rgit-main/src/repo.rs)**: Resolves every path inside `.git`. **Never open `.git/...` paths directly**: wrap them in `crate::repo::path(".git/...")`, which returns them unchanged in a main worktree and routes them to the shared `.git` (objects, refs, config, logs/refs, STASH_LIST) or to `.git/worktrees/<id>/` (HEAD, index, MERGE_*/REBASE_*/BISECT_* state) inside a linked worktree.
7. **[bin/rtest.rs](file:///c:/rgit-main/src/bin/rtest.rs)**: Test harness that cleans and targets `test-sandbox/` for all manual or agentic testing.

---

## 3. Best Practices for Implementation

- **Error Handling**: Use `anyhow::Result` and add context with `.context()` or `.with_context()` to help debug file system issues.
- **Branch Validation**: All branch creation and renaming must pass through `refs::is_valid_branch_name` to prevent formatting conflicts (following `git-check-ref-format`).
- **Graph Walks**: Use the BFS-based `is_reachable` ancestor walk in `helpers.rs` rather than assuming linear history (handles branched or merged histories).
- **Nested Directory Handling**: When deleting branch refs, clean up parent directories under `.git/refs/heads/` recursively if they become empty (so nested namespaces like `feature/login` don't leave empty folder structures).
- **Index Sorting**: The `.git/index` file must have entries sorted alphabetically by path. Use `BTreeMap` or sort vectors before serialization.

### Helper Functions vs. Methods:
- **Methods on Types (`impl Type`) in `objects.rs`**: Use methods for pure data transformations that do not perform disk I/O, process spawning, or repository lookups (e.g., `Action::parse`, `Action::name`, `TodoItem::format_line`, `CleanMode::wants`).
- **Standalone Helper Functions in `helpers/`**: Use standalone functions in `helpers/` when interacting with the file system, repository metadata, state files, child processes, or multi-step graph algorithms.

### Comment Requirements for Helper Functions:
All helper functions in `helpers/` must have a comment written above them explaining what they do and for which feature they are needed. The comments must follow this format:
```rust
/// One or more lines explaining what the helper function does.
// Used by `<command_or_caller>`.
pub fn function_name(...) -> anyhow::Result<...> { ... }
```

### Comment Requirements for Objects:
Every struct, enum, and type alias in `objects.rs` must have descriptive documentation comments directly preceding its declaration containing:
1. **Description of its use case**: A doc-comment (`///`) detailing what data the object models and why it exists.
2. **Used by a file**: A line comment (`// Used by: <file(s)>`) listing the files where the type is used.
3. **Used by command or helper**: A line comment (`// Used by command/helper: <command_or_helper>`) explaining whether it is consumed by a command, a helper, or both.

Example:
```rust
/// An instruction action assigned to an individual commit entry in an interactive rebase todo file.
// Used by: src/helpers/rebase.rs, src/commands/rebase.rs
// Used by command/helper: `parse_edited_todo`, `render_todo_for_editing`, `commit_item`, and the `rebase` command
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Pick,
    Reword,
    ...
}
```
