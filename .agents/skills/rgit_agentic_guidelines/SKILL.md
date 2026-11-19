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
├── commands.rs               # Root module declaring the command modules (no re-exports)
├── commands/                 # High-level CLI command entry points
│   ├── plumbing.rs           # init, hash-object, cat-file, write-tree, ls-tree
│   ├── commit.rs             # commit (-m/-F/-C/-c, -a, --amend, editor, --author/--date, -s), commit-tree
│   ├── log.rs                # log (pretty formats, --date, --decorate, -p/--stat/--name-status/--raw, notes, --graph, --follow), show (commits, tags, trees, blobs), whatchanged
│   ├── staging.rs            # add, status (long, -s/--short, -b, --porcelain[=v1], -z, -u[no|normal|all]), rm, mv
│   ├── patch.rs              # patch mode entry points: add -p, reset -p, checkout -p, restore -p, stash -p
│   ├── branch.rs             # branch (list -a/-r/-v/-vv/patterns/--merged/--contains/--points-at, create -f/-t/--no-track, -d/-D/-r, -m/-M, -c/-C, -u, --unset-upstream, --show-current), switch, checkout
│   ├── restore.rs            # restore (worktree and staged paths)
│   ├── diff.rs               # diff (porcelain on the raw diff engine: worktree/index/commits, ranges, all formats, renames)
│   ├── merge.rs              # merge, merge-base
│   ├── reset.rs              # reset
│   ├── conflicts.rs          # conflicts (rgit-only: unresolved files, who changed each side, --explain <file> side by side)
│   ├── clean.rs              # clean
│   ├── cherry_pick.rs        # cherry-pick (commit lists/ranges, -x, -s, -n, -m, --ff, --continue, --skip, --abort, --quit)
│   ├── revert.rs             # revert (--continue, --abort)
│   ├── stash.rs              # stash (push, pop, apply, list, drop, show, clear)
│   ├── bisect.rs             # bisect (start, bad, good, skip, reset, log, run)
│   ├── tag.rs                # tag (list/filters/-n, lightweight + annotated, -f, -d), describe (--tags/--always/--long/--abbrev/--exact-match)
│   ├── config.rs             # config (get, set, unset, list)
│   ├── rebase.rs             # rebase (--onto, --keep-base, <branch>, --reapply-cherry-picks, -i, --continue, --abort, --skip, --edit-todo)
│   ├── banana_split.rs       # banana-split (splits commit by path/dir into atomic commits)
│   ├── push.rs               # push (local transport & GitHub Git Data API: object transfers, ff checks, ref updates)
│   ├── fetch.rs              # fetch (local transport & GitHub Git Data API: download objects, update remote-tracking refs)
│   ├── clone.rs              # clone (local transport & GitHub Git Data API: init + fetch + checkout + upstream setup)
│   ├── pull.rs               # pull (fetch from remote + merge upstream branch into current branch)
│   ├── remote.rs             # remote (list, verbose, add, remove/rm, rename, get-url, set-url, show)
│   ├── blame.rs              # blame (line-by-line commit attribution, -L ranges, uncommitted changes), annotate
│   ├── archive.rs            # archive (tar, tar.gz/tgz, zip archives with path filters and prefix)
│   ├── reflog.rs             # reflog (show, exists, delete, expire)
│   ├── gc.rs                 # gc (reflog expiry + unreachable loose object pruning, --dry-run)
│   ├── prune.rs              # prune (-n, -v, --expire, <head>...)
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
│   ├── show_ref.rs           # show-ref (patterns, --head, --heads/--tags, -d, -s/--hash, --abbrev, --verify, --exists, --exclude-existing)
│   ├── for_each_ref.rs       # for-each-ref (formats/atoms, --sort, --count, filters, quoting, --color, --stdin)
│   ├── cherry.rs             # cherry (-v, --abbrev[=<n>], <upstream> [<head> [<limit>]])
│   ├── bundle.rs             # bundle (create [-q] [--version], verify [-q], list-heads, unbundle)
│   ├── merge_tree.rs         # merge-tree (--name-only, --[no-]messages, -z, --merge-base, --allow-unrelated-histories, --explain)
│   ├── worktree.rs           # worktree (add, list, remove, move, lock, unlock, prune)
│   ├── patch_id.rs           # patch-id (--stable, --unstable, --verbatim)
│   ├── mailsplit.rs          # mailsplit (-o<dir>, -d<prec>, -f<n>, -b, --keep-cr, --mboxrd, mbox/Maildir/stdin)
│   ├── mailinfo.rs           # mailinfo (-k, -b, -m, -u/-n/--encoding, --[no-]scissors, --no-inbody-headers, --quoted-cr)
│   ├── interpret_trailers.rs # interpret-trailers (--trailer, --where, --if-exists, --if-missing, --parse, --in-place, ...)
│   ├── check_mailmap.rs      # check-mailmap (--stdin, --mailmap-file, --mailmap-blob, <contact>...)
│   ├── fmt_merge_msg.rs      # fmt-merge-msg (-m, --into-name, --log[=<n>], --no-log, -F)
│   ├── request_pull.rs       # request-pull ([-p] <start> <url> [<end>])
│   ├── replace.rs            # replace (<object> <replacement>, --graft, --edit, -d, -l, --format)
│   ├── notes.rs              # notes (list, add, append, edit, copy, show, remove, prune, merge, get-ref; --ref)
│   ├── show_branch.rs        # show-branch (-a, -r, --current, --more, --list, --independent, --merge-base, --topics, --sparse, ...)
│   ├── range_diff.rs         # range-diff (--creation-factor, --left-only, --right-only, -s; two ranges, A...B, base tip1 tip2)
│   ├── last_modified.rs      # last-modified (-r, -t, -z, <revision-range>, paths)
│   ├── repo_info.rs          # repo (info, structure)
│   ├── hook.rs               # hook run (--ignore-missing, --to-stdin)
│   ├── maintenance.rs        # maintenance (run, register, unregister), for-each-repo (--config, --keep-going)
│   ├── replay.rs             # replay (--onto | --advance, --contained, ranges)
│   ├── rerere.rs             # rerere (record/replay, clear, forget, diff, status, remaining, gc)
│   ├── name_rev.rs           # name-rev (--name-only, --tags, --refs, --exclude, --all, --annotate-stdin)
│   ├── refs_cmd.rs           # refs (list, exists, verify, optimize)
│   ├── var.rs                # var (-l, <variable>)
│   ├── stripspace.rs         # stripspace (-s, -c)
│   ├── column.rs             # column (--mode, --width, --padding, --indent, --nl)
│   ├── version.rs            # version (--build-options)
│   ├── help.rs               # help (-a, <command>)
│   ├── read_tree.rs          # read-tree (--empty, --prefix, -m 1/2/3 trees, --reset, -u, -i, -n)
│   ├── update_index.rs       # update-index (--add, --remove, --cacheinfo, --chmod, --refresh, --index-info, --stdin)
│   ├── checkout_index.rs     # checkout-index (-a, -f, -n, -u, --prefix, --temp, --stdin)
│   ├── diff_tree.rs          # diff-tree (-r, -t, --root, -m, --stdin, -v/--pretty, common diff options)
│   ├── diff_index.rs         # diff-index (--cached, -m, common diff options)
│   ├── diff_files.rs         # diff-files (common diff options)
│   ├── merge_file.rs         # merge-file (-p, --diff3, --zdiff3, --ours/--theirs/--union, --object-id, -L)
│   ├── mktree.rs             # mktree (-z, --missing, --batch)
│   ├── mktag.rs              # mktag (--no-strict)
│   ├── unpack_file.rs        # unpack-file <blob>
│   ├── get_tar_commit_id.rs  # get-tar-commit-id
│   ├── repack.rs             # repack (-a, -A, -d, --window, --depth)
│   ├── pack_objects.rs       # pack-objects (--revs, --all, --unpacked, --stdout | <base-name>)
│   ├── unpack_objects.rs     # unpack-objects (-n, -r)
│   ├── index_pack.rs         # index-pack (<pack>, -o, --stdin, --fix-thin, --keep, --verify)
│   ├── verify_pack.rs        # verify-pack (-v, -s)
│   ├── show_index.rs         # show-index
│   ├── prune_packed.rs       # prune-packed (-n)
│   ├── commit_graph.rs       # commit-graph (write, verify)
│   ├── multi_pack_index.rs   # multi-pack-index (write, verify, expire)
│   ├── fast_export.rs        # fast-export (revs, marks, --no-data, --full-tree, --signed-tags)
│   ├── fast_import.rs        # fast-import (--force, --stats, --done, marks)
│   ├── sparse_checkout.rs    # sparse-checkout (init, set, add, list, reapply, disable, check-rules)
│   └── submodule.rs          # submodule (add, status, init, update, deinit, foreach, sync, set-url, set-branch)
├── helpers.rs                # Root module declaring the helper modules (no re-exports)
├── helpers/                  # Lower-level helpers & engines
│   ├── checkout.rs           # Working tree sync, switch safety checks, index resets
│   ├── diff.rs               # Myers diff algorithm & unified diff formatting
│   ├── gitignore.rs          # Gitignore parsing and pattern matching
│   ├── merge.rs              # merge bases/commits; merge result → worktree + index stages 1-3; unmerged-index refusals; reset --merge / state cleanup
│   ├── sequencer.rs          # .git/sequencer state (todo, head, abort-safety, opts) and -x / -s trailers for cherry-pick
│   ├── status.rs             # status --short rendering (XY codes, path quoting, -z), untracked dir collapsing, ## branch header
│   ├── narrate.rs            # Narration support: repo snapshots, tree/line counts, last change per path, conflict hunks, risk scan, suggestions
│   ├── branch.rs             # branch arg parsing, list rendering (-v/-vv tracking), upstream config, branch config section moves
│   ├── state_files.rs        # In-progress operation state files, stash list storage and apply/pop
│   ├── clean.rs              # Directory walker for untracked/ignored cleanup candidates
│   ├── banana_split.rs       # Path grouping helpers for banana-split
│   ├── rebase.rs             # Rebase engine: git's commit selection, up-to-date check, todo editor, replay runner
│   ├── push.rs               # Remote repo resolution, object enumeration, loose object copier, ref updates
│   ├── github.rs             # GitHub Git Data API client, blob/tree/commit uploader, remote ref updates
│   ├── fetch.rs              # Local filesystem fetch transport, remote branch walking, object downloading
│   ├── clone.rs              # Source classification, skeleton repo creation, target dir prep/cleanup, worktree checkout
│   ├── remote.rs             # Remote config queries, name validation, ref directory manipulation, tracking branch collection
│   ├── blame.rs              # Line attribution algorithm, diff-mapping, tree lookups, date formatting
│   ├── archive.rs            # Archive packing, tar/tar.gz/zip streams, timestamp preservation, path filters
│   ├── gc.rs                 # Reachability roots, unreachable loose object detection, pruning
│   ├── prune.rs              # --expire parsing, stale temp/stray file discovery in .git/objects, empty fan-out cleanup
│   ├── fsck.rs               # Object integrity checks, connectivity walks, dangling object reports
│   ├── grep.rs               # Matcher construction, pathspec filtering, content search
│   ├── count_objects.rs      # Loose/packed object statistics, human-readable sizes
│   ├── shortlog.rs           # Author grouping of reachable commits
│   ├── ls_files.rs           # Worktree-vs-index state, untracked/ignored walker, ls-files pathspec matching
│   ├── check_ignore.rs       # Source/line-tracked ignore rules, deciding-rule lookup, path normalization
│   ├── apply.rs              # Patch parsing (incl. binary), hunk placement, binary deltas, shared check/write, diffstat
│   ├── format_patch.rs       # Commit selection, mail headers, git-compatible diff/hunks, diffstat, binary literals
│   ├── am.rs                 # Mailbox split, header decoding, author-preserving commits, .git/rebase-apply state
│   ├── rev_list.rs           # Arg parsing, git revision-walk simulation, history simplification, topo sort, filters, select_rev_list_commits
│   ├── show_ref.rs           # show-ref option parsing, pattern matching, ref resolution/existence, output, --exclude-existing
│   ├── for_each_ref.rs       # for-each-ref: ref-filter port (option/atom parsing, ref collection, filters, values, if/align stack, quoting, versioncmp)
│   ├── log.rs                # log/show arg parsing, pretty formats, date modes, decorations, diff output layout
│   ├── tag.rs                # Tag listing (loose + packed), tag objects, -n lines, packed tag deletion, describe selection
│   ├── cherry.rs             # Upstream lookup, non-merge commit walks, git-compatible patch IDs, hash abbreviation
│   ├── bundle.rs             # Bundle header read/write, rev/ref resolution, object enumeration, packfile write/read with deltas
│   ├── merge_tree.rs         # Line-level 3-way file merge, in-memory tree merge, conflict stages/messages, virtual bases
│   ├── worktree.rs           # Worktree listing/lookup, admin dirs, checkout into a new worktree, prune/move, list formatting
│   ├── patch_id.rs           # git patch-id port: patch stream splitting, hunk scanning, unstable/stable/verbatim hashing
│   ├── mailsplit.rs          # mbox From-line detection, message splitting, Maildir listing, numbered file output
│   ├── mailinfo.rs           # git mailinfo.c port: headers, RFC 2047, in-body headers, scissors, MIME/base64/QP/flowed, charsets
│   ├── interpret_trailers.rs # git trailer.c port: trailer block detection, trailer.* config, placement rules, commands, in-place
│   ├── mailmap.rs            # .mailmap / mailmap.file / mailmap.blob parsing, map_user, contact splitting
│   ├── fmt_merge_msg.rs      # git fmt-merge-msg.c port: FETCH_HEAD lines, title, tag messages, shortlog credits, branch descriptions
│   ├── request_pull.rs       # ls-remote style ref listing (local/GitHub), matching-ref lookup, range shortlog, tree diffs
│   ├── replace.rs            # refs/replace listing/creation/deletion, graft commits, object editing
│   ├── notes.rs              # Notes refs/trees (git fan-out), notes commits, stripspace, notes merge, note lookup for log/show
│   ├── show_branch.rs        # git show-branch.c port: tip collection, join_revs walk, topo sort, commit naming, column display
│   ├── range_diff.rs         # git range-diff.c port: normalized patches, exact matches, linear assignment, pair output
│   ├── last_modified.rs      # Per-path last-change lookup over a range (recursion, trees, boundary commits)
│   ├── repo_info.rs          # repo info keys, repo structure counts, table/key-value output
│   ├── hook.rs               # Hook lookup (core.hooksPath, executable check) and execution
│   ├── maintenance.rs        # Config path lists, running rgit in other repos, maintenance tasks, maintenance.repo registration
│   ├── replay.rs             # In-memory commit replay onto a new base, update-ref command output
│   ├── rerere.rs             # Conflict normalization and IDs, rr-cache store, MERGE_RR, resolution replay
│   ├── name_rev.rs           # git name-rev.c port: tip ranking, name propagation, ref filters/shortening, stdin annotation
│   ├── refs_verify.rs        # Loose ref name/content checks and packed-refs checks for refs verify
│   ├── var.rs                # Logical variables (identity, editors, pager, default branch, shell, global config)
│   ├── stripspace.rs         # Whitespace cleanup on raw bytes, comment-line prefixing
│   ├── column.rs             # git column.c port: mode parsing, display width, column/row/dense layout
│   ├── help.rs               # Command list and per-command help lookup (clap)
│   ├── index_plumbing.rs     # Entry vs worktree state, path normalization, add_index_entry rules, refresh, temp files
│   ├── read_tree.rs          # unpack-trees rules: plain read, --prefix, one/two/three-way merges, worktree checks
│   ├── diff_plumbing.rs      # Raw diffs: tree walk, tree/index/worktree comparison, raw/name/stat/patch rendering
│   ├── merge_file.rs         # merge-file argument parsing
│   ├── mktree.rs             # ls-tree line parsing and checks, tree serialization
│   ├── mktag.rs              # Standalone tag fsck (headers, ident) with git's message ids
│   ├── chunk_file.rs         # Chunk file format shared by commit-graph and multi-pack-index
│   ├── commit_graph.rs       # Commit-graph writer and verifier
│   ├── midx.rs               # Multi-pack-index writer, parser and verifier
│   ├── fast_export.rs        # fast-import stream writer (ref naming, commit order, marks)
│   ├── fast_import.rs        # fast-import stream reader (marks, branch trees, commits, tags, refs)
│   ├── sparse.rs             # Sparse checkout patterns, cone files, apply to worktree + skip-worktree bits, re-apply hook
│   ├── submodule.rs          # .gitmodules, nested repo HEAD, URL resolution, running rgit in a submodule
│   ├── graph.rs              # git graph.c port: log --graph rows, column mapping, merge layouts
│   ├── rename.rs             # git diffcore-rename port: exact/basename/similarity rename+copy detection, -M/-C parsing, limit warnings
│   ├── combine_diff.rs       # git combine-diff.c port: diff --cc of unmerged paths vs the working tree
│   └── follow.rs             # log --follow: rename/copy source search, similarity scoring, rename-aware diff output
│
├── ui.rs                     # Narration layer (C.1/C.10): verbosity levels, before/after snapshot + exit hook, ALL narration wording (stderr only)
├── core.rs                   # Root module declaring the core modules
├── core/                     # Repository fundamentals every domain builds on (no command entry points)
│   ├── check_ref_format.rs   # Git ref name rules, slash normalization, branch shorthand validation
│   ├── commit.rs             # Commit building, parsing, parent chains, ancestor walks (is_reachable)
│   ├── config.rs             # Local & global .gitconfig reader/writer/author identity
│   ├── index.rs              # Binary staging index (.git/index) reader and writer
│   ├── object_store.rs       # Object compression, hashing, tree writing, and refs/replace substitution on read
│   ├── pack.rs               # Packfile engine: idx parse/write, packed object lookup (read_object fallback), deltas, pack writer
│   ├── reflog.rs             # Reflog append/parse, <ref>@{n} resolution, entry deletion and expiry
│   ├── refs.rs               # Reference resolution, branch refs, and HEAD state transitions
│   ├── repo.rs               # Repository path resolution (`repo::path(".git/...")`), main vs linked worktree
│   ├── rev_parse.rs          # Revision syntax resolution, worktree root discovery, property queries
│   └── wildmatch.rs          # git wildmatch() port (WM_PATHNAME, WM_CASEFOLD)
│
├── objects.rs                # Special types, enums, structs, and type aliases
└── bin/
    └── rtest.rs              # Isolated test runner executing rgit within test-sandbox/
```

### Module Responsibilities:

1. **[cli.rs](file:///c:/rgit-main/src/cli.rs)**: Parses and routes all command-line arguments using `clap`. All subcommands are declared here.
2. **[commands/](file:///c:/rgit-main/src/commands)**: Contains *only* high-level command entry points. Functions here orchestrate workflows between `helpers`, `refs`, and `index`, and must return `anyhow::Result<()>`. They do not contain low-level helper logic.
3. **[helpers/](file:///c:/rgit-main/src/helpers)**: Standalone operational functions that read/write compressed objects, execute file I/O, spawn editors, walk commit graphs, manage state files, and resolve merge conflicts.
4. **[objects.rs](file:///c:/rgit-main/src/objects.rs)**: Single source of truth for all domain structures, enums, AST items, and shared type aliases used across commands and helpers.
5. **[core/refs.rs](file:///c:/rgit-main/src/core/refs.rs)**: Manages reference resolution (`HEAD`, branch heads under `refs/heads/`). Handles attached/detached HEAD state transitions.
6. **[core/index.rs](file:///c:/rgit-main/src/core/index.rs)**: Reads and writes the staging index (`.git/index`).
7. **[core/repo.rs](file:///c:/rgit-main/src/core/repo.rs)**: Resolves every path inside `.git`. **Never open `.git/...` paths directly**: wrap them in `crate::core::repo::path(".git/...")`, which returns them unchanged in a main worktree and routes them to the shared `.git` (objects, refs, config, logs/refs, STASH_LIST) or to `.git/worktrees/<id>/` (HEAD, index, MERGE_*/REBASE_*/BISECT_* state) inside a linked worktree.
7. **[bin/rtest.rs](file:///c:/rgit-main/src/bin/rtest.rs)**: Test harness that cleans and targets `test-sandbox/` for all manual or agentic testing.

---

## 3. Best Practices for Implementation

- **Error Handling**: Use `anyhow::Result` and add context with `.context()` or `.with_context()` to help debug file system issues.
- **Branch Validation**: All branch creation and renaming must pass through `refs::is_valid_branch_name` to prevent formatting conflicts (following `git-check-ref-format`).
- **Core vs. Helpers**: `core/` holds the repository fundamentals (object store, packs, index, refs, repo paths, config, reflog, commits, revision parsing, ref-name rules, wildmatch). Code in `core/` must not depend on `commands/` or feature helpers; when a feature helper grows something every domain needs, move it into `core/`.
- **Explicit Imports**: Import items from the module that defines them (`use crate::core::object_store::read_object;`, `crate::commands::merge::merge(...)`). Never add `pub use x::*` re-exports to `helpers.rs` or `commands.rs`, and never `#![allow(unused_imports)]` to hide them.
- **Graph Walks**: Use the BFS-based `is_reachable` ancestor walk in `core/commit.rs` rather than assuming linear history (handles branched or merged histories).
- **Nested Directory Handling**: When deleting branch refs, clean up parent directories under `.git/refs/heads/` recursively if they become empty (so nested namespaces like `feature/login` don't leave empty folder structures).
- **Index Sorting**: The `.git/index` file must have entries sorted alphabetically by path. Use `BTreeMap` or sort vectors before serialization.

### Narration (src/ui.rs)
- rgit's own explanations go through `crate::ui` (`note`, `detail`, `warn`, `danger`) and are written to **stderr** only; never `println!` prose from a command.
- Levels: `quiet` (git-identical) / `normal` / `explain` / `teach`, from `rgit --plain|--explain|--teach`, `RGIT_VERBOSITY`, or `rgit.verbosity`. Default is `normal` only when stderr is a terminal, so piped runs and parity tests are unchanged; set `RGIT_VERBOSITY=quiet` in test scripts to be explicit.
- New narration belongs in the `report_*` functions in ui.rs (they compare the before/after `RepoSnapshot`), not inside command code.
- Never call `std::process::exit` in rgit code: use `crate::ui::exit(code)`, so the narration knows how the command ended (`src/bin/` tools are the exception).
- Git-style conflict `hint:` blocks are wrapped in `if !crate::ui::narration_replaces_hints()`; do the same for new hints the narration already explains.

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
