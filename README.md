# rgit

A lightweight, fully functional Git implementation written in Rust.

`rgit` was built from scratch to explore the internal architecture and mechanics of version control systems. It features a complete implementation of the core Git object model (blobs, trees, commits), index staging (`.git/index`), reference tracking (`refs/heads/`, `refs/remotes/`, `refs/tags/`, `HEAD`), Myers diff algorithm, 3-way tree merges with conflict markers, interactive rebase, stashes, binary-search debugging (`bisect`), dual remote transports (local filesystem + GitHub REST API), and full push/fetch/clone/pull workflows.

---

## Modular Architecture (v2.1)

`rgit` enforces strict separation of concerns across its codebase:

- **[`src/cli.rs`](file:///c:/rgit-main/src/cli.rs)**: CLI argument parsing, subcommands, and flags powered by `clap`.
- **[`src/commands/`](file:///c:/rgit-main/src/commands)**: Porcelain & plumbing command dispatchers and orchestration workflows.
- **[`src/helpers/`](file:///c:/rgit-main/src/helpers)**: Standalone operational engines:
  - `objects.rs`: Zlib compression/decompression, SHA-1 hashing, and tree writing.
  - `commit.rs`: Commit parsing, parent chains, and BFS reachability walks.
  - `diff.rs`: Myers diff algorithm and unified diff formatting.
  - `merge.rs`: Three-way tree merge algorithm and conflict marker generation.
  - `rebase.rs`: Interactive rebase engine, todo editor, and commit replay.
  - `push.rs` / `fetch.rs`: Object enumeration, missing object pack/copy, and ref updates.
  - `github.rs`: GitHub Git Data API integration (blobs, trees, commits, refs).
  - `clone.rs`: Skeleton creation, remote setup, and initial worktree checkout.
  - `remote.rs`: Remote configuration queries, tracking branch management, and ref directory maintenance.
  - `blame.rs`: Line attribution algorithm, diff-mapping, tree lookups, and date formatting.
  - `archive.rs`: Packing tree structures into tar, compressed tar.gz, and zip archives with prefix and path filtering.
  - `config.rs`: INI parser/writer for local (`.git/config`) and global (`~/.rgitconfig`).
  - `checkout.rs`: Working tree sync, switch safety checks, and index resets.
  - `gitignore.rs`: `.gitignore` parsing, glob matching, and ignore-aware file collection.
  - `state_files.rs`: In-progress operation state files and stash list storage/apply logic.
  - `clean.rs`: Directory walker for untracked/ignored cleanup candidates.
  - `bisect.rs`: Ancestor collection, candidate ordering, good/bad/skip marking, and bisect checkouts.
  - `banana_split.rs`: Path grouping for per-file / per-directory commit splitting.
  - `reflog.rs`: Reflog append, parsing, `<ref>@{n}` resolution, entry deletion, and expiry.
  - `gc.rs`: Reachability roots, unreachable loose object detection, and pruning.
  - `prune.rs`: `--expire` date parsing, stale temporary / stray file discovery in `.git/objects`, and empty fan-out directory cleanup.
  - `fsck.rs`: Object integrity checks, connectivity walks, and dangling object reports.
  - `grep.rs`: Regex/fixed-string matcher construction, pathspec filtering, and content search.
  - `count_objects.rs`: Loose/packed object statistics and human-readable size formatting.
  - `rev_parse.rs`: Revision syntax resolution (`HEAD~2`, `HEAD^`, `<ref>@{n}`, short hashes) and repository property queries.
  - `shortlog.rs`: Author grouping of reachable commits.
  - `ls_files.rs`: Worktree-vs-index comparison, untracked/ignored file walker, and pathspec matching.
  - `apply.rs`: Patch parsing (git, traditional, mailbox, binary), hunk placement with line-ending preservation, git binary delta decoding, all-or-nothing check/write shared with `am`, and diffstat/numstat/summary formatting.
  - `format_patch.rs`: Commit parsing and selection, RFC 2822/2047 mail headers, git-compatible unified diffs (xdiff hunk compaction, indent heuristic, funcname headers), diffstat, and binary literal encoding.
  - `am.rs`: Mailbox splitting, header decoding, subject cleanup, commit creation with original author, and `.git/rebase-apply/` session state.
  - `log.rs`: `log`/`show` option parsing, pretty formats (`oneline` … `raw`, templates with `%d`/`%D`), date modes (incl. git's relative-date rounding), `--decorate` ordering, and stat/patch/name-status output layout.
  - `tag.rs`: Tag listing (loose and packed), annotated tag objects, `-n` message lines, packed-ref deletion, and git's `describe` candidate selection.
  - `show_ref.rs`: Option parsing, trailing-component pattern matching, ref resolution (loose, symbolic, packed) and raw existence checks, ref/peeled-tag output, and `--exclude-existing` filtering.
  - `for_each_ref.rs`: A port of git's ref-filter: option parsing (abbreviated/negated long options), atom parsing and format verification, ref collection with symref/packed flags and broken-ref warnings, pattern/exclude/points-at/merged/contains filters, per-atom values (objects, people, dates incl. strftime, subject/body/trailers, upstream/push tracking, describe, ahead-behind), the `%(if)`/`%(align)` formatting stack, shell/perl/python/tcl quoting, color specs, and `versioncmp` sorting.
  - `wildmatch.rs`: A port of git's `wildmatch()` glob matcher (`WM_PATHNAME`, `WM_CASEFOLD`, character classes).
  - `rev_list.rs`: Argument parsing, ref collection, a flag-for-flag port of git's revision walk (`limit_list`, `process_parents`, `try_to_simplify_commit`, `rewrite_one`), topological sort, commit filters, and `--format` expansion.
  - `bundle.rs`: Bundle header parsing/writing (v2/v3, prerequisites, refs), revision and ref (`dwim`) resolution, commit/prerequisite selection, object enumeration, packfile writing, and packfile reading with `OFS_DELTA`/`REF_DELTA` resolution (including thin packs).
  - `merge_tree.rs`: Line-level three-way file merge (xdiff zealous level: conflict refinement, joining nearby conflicts, CRLF markers), path-by-path tree merge with git's conflict kinds and messages, file/directory clash handling, and virtual merge bases for criss-cross histories.
  - `cherry.rs`: Upstream lookup from branch config, non-merge commit walks, git-compatible patch IDs (per-file whitespace-stripped hashes summed with carry), and hash abbreviation.
  - `check_ref_format.rs`: Git's ref name rules (component, `..`, `@{`, `.lock`, forbidden bytes, refspec `*`), slash normalization, and branch shorthand checks.
  - `check_ignore.rs`: Source-tracked `.gitignore` rule loading, deciding-rule lookup (including ignored parent directories), and path normalization.
  - `worktree.rs`: Worktree enumeration (HEAD, locked, prunable), path/suffix lookup, `.git/worktrees/<id>/` admin directories, checkout into a new worktree, dirty checks, prune/move, and `list` / `--porcelain` formatting.
  - `patch_id.rs`: A port of git's `patch-id`: patch stream splitting at commit lines, hunk-header skipping, whitespace stripping, and the unstable / stable (per-file, summed with carry) / verbatim SHA-1 variants.
  - `mailsplit.rs`: mbox `From ` separator detection, message splitting (bare mails, CRLF handling, mboxrd unquoting), Maildir listing in numeric file order, and numbered file output.
  - `mailinfo.rs`: A port of git's `mailinfo.c`: header unfolding and RFC 2047 decoding, in-body headers, scissors lines, MIME multipart boundaries, base64 / quoted-printable / `format=flowed` decoding, charset conversion, subject cleanup, and author extraction.
  - `interpret_trailers.rs`: A port of git's `trailer.c`: trailer block detection, `trailer.*` configuration (keys, separators, commands), `--where` / `--if-exists` / `--if-missing` placement rules, unfolding, and in-place rewriting.
  - `mailmap.rs`: `.mailmap` / `mailmap.file` / `mailmap.blob` parsing (all four line forms), case-insensitive name and email mapping, and contact splitting.
  - `fmt_merge_msg.rs`: A port of git's `fmt-merge-msg.c`: `FETCH_HEAD` line parsing, redundant tip removal, title joining by repository, `merge.suppressDest`, tag messages, and the per-tip shortlog with `By` / `Via` credits.
  - `request_pull.rs`: `ls-remote`-style ref listing for local and GitHub repositories, matching-ref lookup, range shortlog with mailmap, and commit-to-commit tree diffs.
  - `replace.rs`: `refs/replace/` listing, deletion and creation with git's type and existence checks, graft commits with rewritten parents, and editing objects (trees as `ls-tree` lines). Object reads honour replacements through `objects.rs`.
  - `notes.rs`: Notes refs and trees (any fan-out when reading, git's fan-out rule when writing), notes commits, `stripspace`, the editor template, automatic notes merges (`ours`, `theirs`, `union`, `cat_sort_uniq`), and note lookup for `log` / `show`.
  - `show_branch.rs`: A port of git's `show-branch.c`: tip collection (globs, `--all`, `--remotes`, `--current`), the flag-propagating `join_revs` walk, topological sorting, tip-relative commit naming, and the column display.
  - `range_diff.rs`: A port of git's `range-diff.c`: normalized per-commit patches, exact matching, the Jonker-Volgenant linear assignment over diff-of-diffs costs, and the pair listing with patch comparisons.
  - `last_modified.rs`: Per-path last-change lookup over a revision range, with recursion, tree entries, path arguments and boundary commits.
  - `repo_info.rs`: `repo info` keys and `repo structure` ref / reachable-object counts with table and key-value output.
  - `hook.rs`: Hook lookup (`core.hooksPath`, executable check) and execution with arguments and standard input.
  - `maintenance.rs`: Multi-valued path config lists, running rgit in other repositories, maintenance task selection and execution, and `maintenance.repo` registration.
  - `replay.rs`: In-memory replay of commit ranges onto a new base (three-way tree merges, preserved authorship) and `update-ref` command output.
  - `rerere.rs`: git's conflict normalization and conflict IDs, the `rr-cache` preimage/postimage store, `MERGE_RR`, resolution replay by three-way merge, and `forget` / `clear` / `gc` / `diff`.
  - `name_rev.rs`: A port of git's `name-rev.c`: ref tips ranked tags-first and oldest-first, names spread to ancestors (`~N`, `^N`) with git's distance rules, `--refs` / `--exclude` sub-path matching, unambiguous ref shortening, and stdin annotation.
  - `column.rs`: A port of git's `column.c`: mode parsing (`always`/`never`/`auto`, `column`/`row`/`plain`, `dense`), display width, and the column/row/dense table layout.
  - `stripspace.rs`: git's whitespace cleanup on raw bytes and the comment-line prefixer.
  - `refs_verify.rs`: Consistency checks for loose ref names and contents and for `packed-refs`.
  - `var.rs`: The logical variables behind `var` (identity, editors, pager, default branch, shell, global config).
  - `help.rs`: Command list and per-command help lookup on top of clap.
  - `index_plumbing.rs`: Index entry vs working-tree comparison (stat data, then content), path normalization, `add_index_entry` rules (`--add`, `--replace`, file/directory clashes), refresh, temporary files, and writing an entry to disk.
  - `read_tree.rs`: git's `unpack-trees` merge rules for `read-tree`: plain read, `--prefix`, one-way, two-way and trivial three-way merges with their working-tree checks.
  - `diff_plumbing.rs`: Raw diff engine: tree-to-tree walk in git's entry order (`-r`, `-t`), tree-to-index and index-to-working-tree comparison, pathspec selection, and raw / name / stat / numstat / summary / patch output with `-z`, `--abbrev` and `-R`.
  - `merge_file.rs`: Argument parsing for `merge-file` (the merge itself lives in `merge_tree.rs`, which gained diff3 / zdiff3 styles and `--ours` / `--theirs` / `--union`).
  - `mktree.rs`: `ls-tree` line parsing with git's type checks and tree serialization in git's entry order.
  - `mktag.rs`: git's standalone tag fsck (`fsck_tag_standalone`, `fsck_ident`) with its message ids.
  - `pack.rs`: Packfile engine: `.idx` v1/v2 parsing, object lookup in the repository's packs (the fallback of `read_object`, with offset/ref delta resolution), pack scanning and delta resolution for indexing, the pack writer with a block-matching delta encoder, `.idx` v2 writing, thin-pack completion, reachable-object enumeration and `prune-packed`.
  - `chunk_file.rs`: git's chunk file format (table of contents, fan-out, trailing checksum) shared by the commit-graph and the multi-pack-index.
  - `commit_graph.rs`: Commit-graph writer (OIDF/OIDL/CDAT/GDA2/GDO2/EDGE, topological levels and corrected commit dates) and verifier.
  - `midx.rs`: Multi-pack-index writer (PNAM/OIDF/OIDL/OOFF/LOFF), parser and verifier.
  - `fast_export.rs`: `fast-import` stream writer: git's ref naming and commit order, blobs, file changes, extra refs, tags and marks.
  - `fast_import.rs`: `fast-import` stream reader: data blocks, marks, in-memory branch trees, commits, tags, resets, and the final ref update.
  - `sparse.rs`: Sparse checkout: pattern file and config switches, `.gitignore`-style matching with git's walk-up rule (covers cone and non-cone patterns), cone pattern generation, applying the patterns to the working tree and the skip-worktree bits, and the re-apply hook run after working-tree commands.
  - `graph.rs`: A port of git's `graph.c`: the `log --graph` state machine (padding, skip, pre-commit, commit, post-merge and collapsing rows), column mapping, merge layouts and octopus dashes.
  - `rename.rs`: Rename/copy detection (git's `diffcore-rename` + `diffcore-delta` similarity), option and `diff.renames` parsing, `pprint_rename`, rename-limit warnings.
  - `follow.rs`: `log --follow`: per-commit diff of the followed path, git's rename/copy source search (`diffcore-delta` chunk hashing, the four-candidate table, renames before copies) and rename-aware `--name-status` / `--raw` / `--stat` / `-p` rendering.
  - `submodule.rs`: Submodules: `.gitmodules` parsing, a nested repository's git directory and `HEAD`, URL resolution, running rgit inside a submodule, and the `Subproject commit` stand-in for diffs.
- **[`src/index.rs`](file:///c:/rgit-main/src/index.rs)**: Binary staging index parser and serializer (`.git/index`).
- **[`src/refs.rs`](file:///c:/rgit-main/src/refs.rs)**: Reference resolution (`HEAD`, branch heads, symbolic refs, detached HEAD).
- **[`src/repo.rs`](file:///c:/rgit-main/src/repo.rs)**: Repository path resolution: maps `.git/...` paths to the shared `.git` or to a linked worktree's `.git/worktrees/<id>/`.
- **[`src/objects.rs`](file:///c:/rgit-main/src/objects.rs)**: Domain models, shared type aliases, and CLI subcommand enums.
- **[`src/bin/rtest.rs`](file:///c:/rgit-main/src/bin/rtest.rs)**: Isolated sandbox test harness executing `rgit` in `test-sandbox/`.

---

## Features & Supported Commands

### Repository & Staging

- **`init`**: Initializes a new repository with standard `.git/` directory layout.
- **Patch mode** (`add -p`, `reset -p [<tree-ish>]`, `checkout -p [<tree-ish>]`, `restore -p [-S] [-W] [--source=<tree>]`, `stash [push] -p [-m <msg>] [-- <paths>]`): A port of git's `add-patch.c`. Shows each file's diff hunk by hunk with git's prompts (`Stage this hunk`, `Unstage this hunk`, `Discard this hunk from worktree`, `Apply this hunk to index and worktree`, `Stash this hunk`, ... and the mode-change / deletion / addition variants) and commands `y n q a d j J k K g / s e ?`: split a hunk into its change groups, edit it in the editor (with git's instructions; re-edit when it does not apply), go to a hunk by number or regex. Each file's chosen hunks are applied as soon as it is done, with git's placement rules (split hunks merged back, start/end anchoring). `stash -p` stashes only the chosen hunks, takes them out of the working tree, and keeps the index (git's implied `--keep-index`). Answers are read line by line (git's default, `interactive.singleKey` off), so it can be scripted by piping answers in.
- **`add <paths...>`**: Hashes file contents into blobs and updates the binary index (`.git/index`).
- **`rm <files...>`** (`--cached`, `-r`, `-f`): Removes files from index and/or working tree.
- **`mv <source> <destination>`** (`-f`): Moves/renames files, staging the change in the index.
- **`status`**: Reports untracked files, working-tree modifications, and staged changes.
- **`restore`** (`--staged`, `--worktree`, `--source`): Restores working tree or index files from a tree or commit.
- **`clean`** (`-f`, `-d`, `-x`, `-X`): Prunes untracked and ignored files with `.gitignore` pattern evaluation.

### History, Inspection & Diffs

- **`commit`** (`-m <msg>`..., `-F <file>`, `-C|-c <commit>`, `-a`, `--amend`, `-e|--no-edit`, `--author=<who>`, `--date=<when>`, `--reset-author`, `--allow-empty`, `--allow-empty-message`, `-s`, `-q`): Records the index as a commit. The message comes from `-m` (several become paragraphs), a file, another commit, or the editor (`core.editor`/`$EDITOR`, `#` lines stripped), which opens when none is given and to conclude a merge; `-a` stages tracked changes first, `--amend` replaces the tip keeping its parents and author, and `-s` adds a `Signed-off-by:` trailer. Refuses empty commits and messages unless allowed, and refuses to conclude a merge while conflict markers remain. Output, reflog entries (`commit:`, `commit (initial|amend|merge):`) and stored commits match git's, including the diffstat and create/delete-mode summary. Timestamps are recorded in UTC.
- **`log`** (`[<revision-range>] [[--] <path>...]`, every `rev-list` walk option (`-n`, `--skip`, `--since`/`--until`, `--author`, `--grep`, `--all`, `--first-parent`, `--merges`/`--no-merges`, `--reverse`, `--topo-order`, path limiting), `--oneline`, `--pretty=oneline|short|medium|full|fuller|raw|format:|tformat:`, `--format=<template>` (incl. `%d`/`%D`), `--date=default|iso|iso-strict|short|raw|unix|rfc|relative`, `--decorate[=short|full|no]`/`--no-decorate`, `-p`, `--stat`, `--shortstat`, `--name-only`, `--name-status`, `-s`): Shows history with output identical to git's (separators, `Merge:` lines, decoration order, stat/patch layout); decorations are on by default on a terminal or via `log.decorate`. `--graph` draws git's ASCII commit graph (merges, octopus merges, collapsing lines; implies `--topo-order`), and `--follow <path>` follows one file across renames and copies with git's similarity scoring (`R`/`C` with percentage in `--name-status`, `old => new` in `--stat`, `rename from`/`to` in `-p`).
- **`shortlog`** (`-s|--summary`, `-n|--numbered`, `-e|--email`, `--all`, `[<rev>]`): Summarizes history grouped by author, with commit counts and subject lines.
- **`grep`** (`-i`, `-v`, `-n`, `-c`, `-l|-L`, `-F`, `--cached`, `-e <pattern>`, `<pattern> [<rev>...] [-- <path>...]`): Searches tracked files, the index, or historical revisions using regex or fixed-string patterns.
- **`show`** (`[<object>...]`, `log`'s display options, `-- <path>...`): Shows commits (with their patch; `--stat`/`--name-status` replace it), annotated tags (tagger, date and message, then the tagged object), trees (entry list) and blobs (contents), including `<rev>:<path>` objects, exactly as git prints them.
- **`diff`** (`[--cached|--staged] [<commit> [<commit>]]`, `A..B`, `A...B`, `[--] <path>...`, `--stat`, `--shortstat`, `--numstat`, `--summary`, `--name-only`, `--name-status`, `--raw`, `-s`, `-z`, `-R`, `--abbrev`, `--exit-code`, `--quiet`): git's porcelain diff on the raw diff engine: working tree vs index, index vs `HEAD`/a commit, working tree vs a commit, or two commits (from their merge base with `...`). Renames are detected by default (see *Rename detection*).
- **Rename detection** (`-M[<n>]`, `-C[<n>]`, `--find-renames[=<n>]`, `--find-copies[=<n>]`, `--find-copies-harder` / `-C -C`, `-l<n>`, `--no-renames`; config `diff.renames` = true/false/copies, `diff.renameLimit`, `status.renames`): a port of git's `diffcore-rename`: exact renames, the same-basename shortcut (75%), the four-candidate similarity table scored like `diffcore-delta`, renames before copies, and git's rename-limit fallback and warnings. On by default in `diff`, `log`/`show`/`whatchanged` and `status` (`renamed:    a -> b`); opt-in for `diff-tree`/`diff-index`/`diff-files` and `fast-export` (`R`/`C` commands). Shown as `R086 old new` in raw / `--name-status`, `old => new` (`{a => b}` form) in `--stat`/`--numstat`, ` rename ... (86%)` in `--summary`, and `similarity index` / `rename from` / `rename to` (or `copy`) in patches.
- **`apply`** (`--check`, `--stat`, `--numstat`, `--summary`, `--apply`, `--cached`, `--index`, `-R`, `-v`, `-p<n>`, `[<patch>...]`): Applies unified or git-style diffs (including `format-patch` mail) to the working tree and/or index, all-or-nothing, with offset hunk placement, creations, deletions, renames, copies, mode changes, `GIT binary patch` literals and deltas, and CRLF/no-final-newline preservation.
- **`format-patch`** (`-<n>`, `--root`, `--stdout`, `-o <dir>`, `-n|-N`, `--start-number`, `--subject-prefix`, `-k`, `--signature`/`--no-signature`, `<since>|<A..B>`): Exports commits as mailbox patches (`NNNN-subject.patch`) with RFC 2047 headers, diffstat and summary, git-identical hunks (function-name headers, indent heuristic), and binary literals.
- **`merge-tree`** (`[--write-tree]`, `--name-only`, `--[no-]messages`, `-z`, `--merge-base=<tree-ish>`, `--allow-unrelated-histories`, `--explain`, `<branch1> <branch2>`): Merges two commits entirely in memory and writes only the merged tree (conflicted files contain markers), leaving refs, index and working tree untouched. Default output matches git's: tree hash, conflicted stages (`<mode> <oid> <stage>	<path>`), and messages (`Auto-merging`, `CONFLICT (content|add/add|modify/delete|file/directory)`, binary warnings); exits 0 if clean, 1 on conflicts. `--explain` prints a readable report instead: the ours/theirs/base commits (including virtual bases), a per-path table of what each side did and what the merge decided, and a base/ours/theirs preview of every conflicting region. Renames are not detected.
- **`cherry`** (`-v`, `--abbrev[=<n>]`, `[<upstream> [<head> [<limit>]]]`): Lists the commits in `<head>` (default `HEAD`) missing from `<upstream>` (default the branch's upstream), oldest first, marking each `-` if an equivalent change already exists upstream (same patch ID as git computes it, so cherry-picks and whitespace-only differences are recognized) or `+` if it doesn't; merges are ignored and `<limit>` cuts off older commits.
- **`am`** (`[<mbox>...]`, `-k`, `--continue`, `--skip`, `--abort`, `--quit`, `--show-current-patch[=raw|diff]`): Applies a mailbox of patches as commits that keep the original author, date, and message; stops on conflicts with resumable state in `.git/rebase-apply/`.
- **`tag`** (`[-l] [-n[<num>]] [--contains <commit>] [--points-at <object>] [<pattern>...]`, `[-a] [-f] [-m <msg>... | -F <file>] [-e] <name> [<object>]`, `-d <name>...`): Lists tags (glob patterns, message lines, containment and points-at filters), creates lightweight or annotated tags on any object (tag objects with tagger and message, editor when `-a` has no message, `#` lines stripped), replaces with `-f` (`Updated tag … (was …)`), and deletes loose or packed tags (`Deleted tag … (was …)`), with git's messages and exit codes.
- **`describe`** (`[--tags] [--always] [--long] [--abbrev=<n>] [--exact-match] [<commit>...]`): Names commits after the nearest tag as `<tag>-<n>-g<hash>` (`<n>` = commits since the tag); annotated tags only unless `--tags`, preferring annotated and newer tags on the same commit, with git's error messages (including the "try --tags" hint).
- **`blame`** (`[-L <start>,<end>] [<rev>] [--] <file>`): Shows line-by-line commit attribution, author info, timestamps, and detects uncommitted lines.
- **`archive`** (`[-f|--format <fmt>]`, `[-l|--list]`, `[--prefix <prefix>]`, `[-o|--output <file>]`, `<tree-ish> [<paths...>]`): Exports files from a tree, commit, tag, or ref into a `tar`, `tar.gz`/`tgz`, or `zip` archive with directory hierarchy and commit timestamps preserved.
- **`annotate`** (`-L <start>,<end>`, `[<rev>] <file>`): `blame` in git's tab-separated `annotate` format.
- **`whatchanged`** (`log`'s options): `log` with the raw change list (`:<old mode> <new mode> <old hash> <new hash> <status>\t<path>`) after each commit, skipping merges and commits that change nothing. `log` and `show` also accept `--raw` now.
- **`show-branch`** (`-a`, `-r`, `--current`, `--more=<n>`, `--list`, `--independent`, `--merge-base`, `--topo-order`, `--date-order`, `--sha1-name`, `--no-name`, `--topics`, `--sparse`, `<rev>|<glob>...`): Shows branch tips and, column by column, which of them contain each commit down to their common ancestor.
- **`range-diff`** (`--creation-factor=<n>`, `--left-only`, `--right-only`, `-s`; `<range1> <range2>`, `<rev1>...<rev2>`, `<base> <rev1> <rev2>`): Compares two versions of a commit range, pairing up commits (`=`, `!`, `<`, `>`) and showing how the changed patches differ.
- **`last-modified`** (`-r`, `-t`, `-z`, `[<revision-range>] [-- <path>...]`): For each path, the commit that last changed it (git 2.52's command; output ordered by path).
- **`notes`** (`--ref <ref>`; `list`, `add`, `append`, `edit`, `copy`, `show`, `remove`, `prune`, `merge -s <strategy>`, `get-ref`): Attaches notes to commits under `refs/notes/commits`, in git's tree layout. `log` and `show` print them in a `Notes:` block and as `%N` (`--notes` / `--no-notes`). Notes merge conflicts can only be resolved with a strategy, not by hand.
- **`replace`** (`[-f] <object> <replacement>`, `--graft <commit> [<parent>...]`, `--edit <object>` (`--raw`), `-d <object>...`, `-l [<pattern>]` (`--format=short|medium|long`)): Manages `refs/replace/` so that reads of an object return its replacement; `fsck`, `gc`, `prune`, `count-objects`, `bundle`, `push`, `fetch`, `clone` and `pull` ignore replacements, as does everything when `GIT_NO_REPLACE_OBJECTS` is set.

### Branching & Navigation

- **`branch`** (`[<name>]`, `-d`, `-D`, `-m`): Lists, creates, renames, and safely deletes branches with ancestor checks.
- **`switch`** (`<branch>`, `-c`, `--detach`, `-f`): Modern branch switching and detached HEAD transitions with worktree safety.
- **`checkout`** (`<branch|commit>`, `-b`, `--detach`, `-f`): Classic branch, commit, and revision switching interface.
- **`bundle`** (`create [-q] [--version=2|3] <file> <rev-list-args>`, `verify [-q] <file>`, `list-heads <file> [<refname>...]`, `unbundle <file> [<refname>...]`): Moves history offline in git's bundle format, so bundles work with real git in both directions (`git clone`/`git fetch` from an rgit bundle, `rgit bundle unbundle` of a git bundle). `create` accepts refs, `^rev`, `A..B`, `A...B`, `--not`, `--all|--branches|--tags|--remotes`, records the prerequisite commits a receiver must already have, and packs every needed object (undeltified, so bundles are larger than git's); `verify` checks prerequisites and describes the bundle; `unbundle` stores the objects as loose objects and prints the refs without updating any.
- **`worktree`** (`add [-b|-B <branch>] [-d] [-f] [--lock [--reason]] <path> [<commit-ish>]`, `list [--porcelain|-v]`, `remove [-f [-f]]`, `move`, `lock [--reason]`, `unlock`, `prune [-n] [-v]`): Manages multiple working trees attached to one repository using git's on-disk layout (`<path>/.git` file → `.git/worktrees/<id>/`), so real git can use them too. Each worktree has its own HEAD, index and in-progress state; objects, refs, config and stashes are shared. A branch can be checked out in only one worktree at a time (`switch`, `checkout` and `branch -d` respect this), `branch -m` updates other worktrees' HEADs, and `gc`/`fsck` treat every worktree's HEAD, index and state files as roots.

### Merging, Replaying & Rebasing

- **`merge <branch>`**: Fast-forwards when possible, otherwise runs the same line-level three-way merge as `merge-tree` (non-overlapping edits to one file merge cleanly; criss-cross histories use a virtual base) and commits the result with the configured identity. Refuses to start while a merge is in progress or when local changes would be overwritten. On conflicts it prints git's messages, writes conflict markers into the working tree, stages everything that merged cleanly, records `MERGE_HEAD`/`MERGE_MSG` (and `ORIG_HEAD`), and exits 1; `rgit commit` concludes the merge once conflicts are resolved and added.
- **`merge-base`** (`<commit> <commit>...`, `-a|--all`, `--is-ancestor`): Finds the best common ancestor(s) of commits, or checks ancestry via exit code.
- **`rebase`** (`<upstream>`, `-i`, `--continue`, `--abort`, `--skip`, `--edit-todo`): Full interactive rebase with `pick`, `reword`, `edit`, `squash`, `fixup`, and `drop` instructions.
- **`cherry-pick <commit>`** (`--no-commit`, `--continue`, `--abort`): Replays a single commit onto current HEAD with state recording.
- **`revert <commit>`** (`--no-commit`, `--continue`, `--abort`): Inverts and applies target commit changes.
- **`replay`** (`--onto <newbase>` | `--advance <branch>`, `--contained`, `<revision-range>...`): Re-creates commits on a new base in the object store only, without touching the working tree, index or refs, and prints `update <ref> <new> <old>` lines for `update-ref --stdin` (git 2.44's command). Exits 1 without output when a commit conflicts.
- **`rerere`** (`clear`, `forget <path>...`, `diff`, `status`, `remaining`, `gc`): Records how conflicts were resolved in `.git/rr-cache` (git's format) and applies the recorded resolution when the same conflict shows up again. Run it by hand after a conflict and again after resolving: rgit's `merge`, `rebase` and `commit` don't call it, and conflicts are detected by their markers.

### State & Recovery

- **`reset`** (`--soft`, `--mixed`, `--hard`, `[-- <paths...>]`): Moves HEAD, restores the index, and synchronizes the working tree.
- **`stash`** (`push`, `pop`, `apply`, `list`, `drop`, `show`, `clear`): Stashes and restores working directory and index state.
- **`reflog`** (`show [<ref>] [-n <count>]`, `exists <ref>`, `delete <ref>@{<n>}...`, `expire [--all] [--expire <days|now|never>] [<ref>]`): Records every move of `HEAD`, branches, and remote-tracking branches so lost commits can be recovered (e.g. `reset --hard HEAD@{1}`).

### Remote Repositories & Collaboration

`rgit` supports two transports for all remote operations:

1. **Local transport**: Any local directory path or bare repository on the filesystem.
2. **GitHub REST API transport**: Remote GitHub repositories (`owner/repo` or HTTPS URL) using Personal Access Tokens.

- **`remote`** (`[-v|--verbose]`, `add`, `remove`/`rm`, `rename`, `get-url`, `set-url`, `show`): Manages tracked remotes, URLs, fetch refspecs, and remote-tracking references.
- **`fetch [<remote>] [<branch>]`**: Walks remote commit graphs, downloads missing objects, updates remote-tracking refs (`refs/remotes/<remote>/<branch>`), and writes `FETCH_HEAD`.
- **`push [<remote>] [<branch>]`** (`-f|--force`, `-u|--set-upstream`): Computes missing objects, transfers blobs/trees/commits, enforces fast-forward checks, and updates remote references.
- **`clone [-b <branch>] [-o <origin>] <repository> [<directory>]`**: Initializes target directory, fetches remote objects, configures remote & upstream tracking, and checks out the working tree.
- **`pull [<remote>] [<branch>]`**: Seamless convenience command composing `fetch` with upstream tracking resolution and `merge`.
- **`request-pull [-p] <start> <url> [<end>]`**: Writes a pull request summary: base commit, repository and ref to pull, tip commit, tag message or branch description, a shortlog of `<start>..<end>` grouped by author (through `.mailmap`), and a diffstat with creations, deletions and mode changes (`-p` adds the patch). Warns (exit 1) when `<url>` has no ref named like `<end>` at that commit. Renames show as delete + create.
- **`fmt-merge-msg`** (`-m <message>`, `--into-name <branch>`, `--log[=<n>]`, `--no-log`, `-F <file>`): Builds the default merge commit message from `FETCH_HEAD`-style lines on standard input: `Merge branch 'x' of <repo> into <branch>` (no ` into` for `main`/`master` or `merge.suppressDest` patterns), merged tag messages, and with `--log` / `merge.log` a per-tip list of new commit subjects with `# By` / `# Via` credit lines.
- **`check-mailmap`** (`--stdin`, `--mailmap-file <file>`, `--mailmap-blob <blob>`, `<contact>...`): Prints the canonical `Name <email>` of each contact according to `.mailmap`, `mailmap.blob` and `mailmap.file`.

### Configuration & Diagnostics

- **`config`** (`[--global]`, `<key> [<value>]`, `--list`, `--unset`): Reads, writes, lists, and unsets local (`.git/config`) and global (`~/.rgitconfig`) configurations.
- **`bisect`** (`start`, `bad`, `good`, `skip`, `reset`, `log`, `run`): Automated binary search to locate the regression-introducing commit.
- **`banana-split <commit>`** (`--by-dir`, `-n|--dry-run`): Custom tool that splits a commit into atomic per-path or per-directory commits.
- **`gc`** (`--prune <days|now|never>`, `--reflog-expire <days|now|never>`, `-n|--dry-run`): Expires old reflog entries and deletes unreachable loose objects past the prune window, keeping anything reachable from refs, the index, the stash, in-progress operations, or live reflogs.
- **`prune`** (`-n`, `-v`, `--expire <time>`, `[--] <head>...`): Deletes every unreachable loose object (git's default; `--expire` keeps newer ones and whatever they refer to), using the same roots as `gc` (refs, HEAD, index, stash, operations in progress, all reflog entries, every worktree) plus any `<head>` given. Also removes stale `tmp_*` files and reports stray files in `.git/objects`. Silent by default; `-n`/`-v` print `<hash> <type>` per object exactly like git. Refuses to touch a damaged repository or one with packfiles.
- **`fsck`** (`--connectivity-only`, `--no-dangling`, `--unreachable`, `--no-reflogs`): Verifies object integrity and connectivity, validates refs and the index, and reports dangling or unreachable objects.
- **`count-objects`** (`-v|--verbose`, `-H|--human-readable`): Reports loose object count and disk usage; verbose mode adds pack, prune-packable, and garbage statistics.
- **`repo`** (`info [--format=keyvalue|nul] [--all | <key>...]`, `structure [--format=table|keyvalue|nul]`): Prints repository facts (`layout.bare`, `layout.shallow`, `object.format`, `references.format`) and counts of refs and reachable objects (git 2.52's command).
- **`hook run`** (`--ignore-missing`, `--to-stdin=<path>`, `<hook-name> [-- <hook-args>]`): Runs a hook script from `.git/hooks` or `core.hooksPath` and exits with its status. rgit's own commands do not fire hooks yet.
- **`for-each-repo`** (`--config=<key>`, `--keep-going`, `<arguments>`): Runs `rgit <arguments>` in every repository listed under a multi-valued config key.
- **`maintenance`** (`run [--auto] [--quiet] [--task=<task>]`, `register`, `unregister [--force]`): Runs upkeep tasks (`gc`, `pack-refs`; the packfile and commit-graph tasks are accepted but skipped) and manages the global `maintenance.repo` list. No `start` / `stop` scheduler.
- **`var`** (`-l`, `<variable>`): Prints rgit's logical variables under git's names: `GIT_AUTHOR_IDENT`, `GIT_COMMITTER_IDENT`, `GIT_EDITOR`, `GIT_SEQUENCE_EDITOR`, `GIT_PAGER`, `GIT_DEFAULT_BRANCH`, `GIT_SHELL_PATH`, `GIT_CONFIG_GLOBAL`; `-l` lists the configuration followed by all of them.
- **`version`** (`--build-options`): Prints `rgit version <version>`, optionally with CPU, OS, pointer size and build profile.
- **`help`** (`-a|--all`, `<command>`): Lists every command in columns, or shows one command's usage and options (the command's own `-h` text where it has one, clap's help otherwise).

### Plumbing Commands

- **`hash-object`** (`-w <file>`): Computes object SHA-1 hash and optionally writes compressed object to store.
- **`cat-file`** (`-p <hash>`): Decompresses and displays object type and contents.
- **`write-tree`**: Serializes current staging index into tree objects.
- **`ls-tree`** (`--name-only <tree>`): Inspects tree contents and modes.
- **`ls-files`** (`-c`, `-m`, `-d`, `-o`, `-i`, `--exclude-standard`, `-s`, `-t`, `-z`, `[<pathspec>...]`): Lists tracked index entries, modified/deleted tracked files, and untracked or ignored working-tree files.
- **`check-ignore`** (`-q`, `-v`, `-n`, `--no-index`, `--stdin`, `-z`, `<pathname>...`): Reports which paths are excluded by `.gitignore` rules; `-v` shows the deciding rule as `<source>:<line>:<pattern>`. Exits 0 if any path is ignored, 1 if none, 128 on usage errors.
- **`check-ref-format`** (`--normalize|--print`, `--[no-]allow-onelevel`, `--refspec-pattern`, `<refname>`; `--branch <name>`): Validates a ref name against git's naming rules, exiting 0 if valid and 1 if not; `--normalize` collapses slashes and prints the result, and `--branch` checks and prints a branch name (exit 128 if invalid). Works outside a repository.
- **`commit-tree <tree> [-p <parent>] -m <msg>`**: Directly writes a commit object from tree and parent hashes.
- **`rev-parse`** (`--git-dir`, `--show-toplevel`, `--is-inside-work-tree`, `--is-bare-repository`, `--abbrev-ref`, `--symbolic-full-name`, `--verify [-q]`, `--short[=N]`, `--sq`, `<rev>...`): Resolves revision syntax (`HEAD~2`, `HEAD^`, `@`, mixed chains like `topic~2^0`, branches, tags, `heads/<name>` / `tags/<name>`, short hashes, `<ref>@{n}`, `^{type}`; a tag wins over a same-named branch, as in git) to object hashes or symbolic names, and queries repository properties.
- **`rev-list`** (`<rev>...`, `^rev`, `A..B`, `A...B`, `--not`, `--all|--branches|--tags|--remotes`, `-n|--max-count`, `--skip`, `--reverse`, `--count`, `--topo-order|--date-order`, `--first-parent`, `--merges|--no-merges|--min-parents|--max-parents`, `--since|--until`, `--author|--committer|--grep` (`-i`, `-F`, `--all-match`, `--invert-grep`), `--parents`, `--timestamp`, `--left-right`, `--abbrev-commit`, `--oneline`, `--format`, `[--] <path>...`): Walks the commit graph exactly like git's revision machinery (date-ordered walk, gradual UNINTERESTING marking, path-limited history simplification with parent rewriting, topological sorting) and prints matching commits.
- **`show-ref`** (`[<pattern>...]`, `--head`, `--heads`, `--tags`, `-d|--dereference`, `-s|--hash[=<n>]`, `--abbrev[=<n>]`, `-q`; `--verify <ref>...`; `--exists <ref>`; `--exclude-existing[=<pattern>]`): Lists loose and packed refs with their objects, matching patterns against whole trailing path components; `-d` adds peeled `^{}` lines for annotated tags, `--verify` checks exact ref names, `--exists` checks one ref without resolving it (exit 2 if missing), and `--exclude-existing` filters refs from stdin down to those missing locally.
- **`symbolic-ref`** (`<name>`, `-q|--quiet`, `--short`, `--[no-]recurse`; `[-m <reason>] <name> <ref>`; `-d|--delete <name>`): Reads, sets and deletes symbolic refs such as `HEAD` or `refs/remotes/origin/HEAD`. With one name it prints the ref it points at (following chains unless `--no-recurse`, shortened with `--short`; unborn targets still print), exiting 1 quietly with `-q` when the ref is not symbolic. With a target it rewrites the ref's own file (HEAD must stay inside `refs/`) and logs the move in its reflog, with `-m` as the message. `--delete` removes a symbolic ref (never HEAD) and its reflog.
- **`update-ref`** (`[-m <reason>] [--no-deref] [--create-reflog] <ref> <new-oid> [<old-oid>]`; `-d <ref> [<old-oid>]`; `--stdin [-z]`): Safely sets or deletes any ref. Values go through revision parsing, an `<old-oid>` makes the change conditional on the current value (empty = must not exist), and symbolic refs like HEAD are followed unless `--no-deref`. Objects must exist and branches only take commits; new refs may not clash with existing ones. Changes are logged with the `-m` message (also in HEAD's reflog for the current branch), and deletions also drop the `packed-refs` entry and the reflog. `--stdin` runs an atomic transaction of `update`/`create`/`delete`/`verify`/`option no-deref` commands, with `start`/`prepare`/`commit`/`abort` for interactive control (C-quoted names, or NUL-separated input with `-z`).
- **`pack-refs`** (`--all`, `--no-prune`, `--include <pattern>`, `--exclude <pattern>`): Moves loose refs into `.git/packed-refs` (git's format, with `^<peeled>` lines for annotated tags). By default only tags are packed; `--all` packs every ref, and `--include`/`--exclude` select refs by glob (excludes win). Symbolic, per-worktree and broken refs stay loose; packed loose files are removed afterwards (keeping their reflogs) unless `--no-prune`.
- **`for-each-ref`** (`[<pattern>...]`, `--format=<format>`, `--sort=<key>` (`-`, `version:`), `--count=<n>`, `--exclude=<pattern>`, `--points-at=<object>`, `--merged`/`--no-merged [<commit>]`, `--contains`/`--no-contains [<commit>]`, `--ignore-case`, `--omit-empty`, `--stdin`, `-s|--shell`, `-p|--perl`, `--python`, `--tcl`, `--color[=<when>]`): Lists refs through `%(atom)` formats with git's atoms — `refname`/`symref` (`:short`, `:lstrip=`, `:rstrip=`), `objectname`/`tree`/`parent` (`:short[=N]`), object type/size, people and dates (all date modes, `format:`), `subject`/`body`/`contents`/`trailers`, `upstream`/`push` (`:track`, `:trackshort`, `:remotename`, `:remoteref`), `HEAD`, `flag`, `worktreepath`, `describe`, `ahead-behind:`, `color:`, `align`, `if`/`then`/`else`/`end`, and `*` to dereference tags. Mailmap options are accepted but not applied, and signatures are reported as unsigned.
- **`patch-id`** (`--stable`, `--unstable`, `--verbatim`): Reads patches on standard input and prints `<patch-id> <commit-id>` for each: a SHA-1 of the diff without line numbers and whitespace, so the same change always gets the same ID. Defaults come from `patchid.stable` / `patchid.verbatim`.
- **`mailsplit`** (`-o<directory>`, `-d<prec>`, `-f<n>`, `-b`, `--keep-cr`, `--mboxrd`, `[<mbox>|<Maildir>...]`): Splits mbox files, Maildirs or standard input into one numbered file per message (`0001`, `0002`, ...) and prints the count.
- **`mailinfo`** (`-k`, `-b`, `-m|--message-id`, `-u`, `-n`, `--encoding=<encoding>`, `--[no-]scissors`, `--no-inbody-headers`, `--quoted-cr=<action>`, `<msg> <patch>`): Reads one email on standard input, writes its commit message and patch to the two files, and prints `Author:` / `Email:` / `Subject:` / `Date:`. Decodes RFC 2047 headers, MIME multipart, base64, quoted-printable and `format=flowed`. Charset conversion covers UTF-8, US-ASCII, ISO-8859-1/15 and Windows-1252; `--encoding` re-codes to the named charset as documented (git 2.43 itself turns re-coding off there).
- **`interpret-trailers`** (`--in-place`, `--trim-empty`, `--trailer <token>[(=|:)<value>]`, `--where <placement>`, `--if-exists <action>`, `--if-missing <action>`, `--only-trailers`, `--only-input`, `--unfold`, `--parse`, `--no-divider`, `[<file>...]`): Adds or parses the `Token: value` trailers at the end of commit messages, honouring `trailer.separators`, `trailer.where`, `trailer.ifexists`, `trailer.ifmissing` and `trailer.<token>.key|command|cmd|where|ifexists|ifmissing`.
- **`name-rev`** (`--name-only`, `--tags`, `--refs=<pattern>`, `--exclude=<pattern>`, `--all`, `--annotate-stdin`, `--no-undefined`, `--always`, `<commit>...`): Names revisions relative to the nearest ref (`main~3`, `tags/v1^0`, `main~3^2~1`) with git's rules (tags first, older tags first, first-parent paths preferred); can list every reachable commit or annotate hashes in text read from standard input.
- **`refs`** (`list ...`, `exists <ref>`, `verify [--strict] [--verbose]`, `optimize ...`): git 2.46+ umbrella command: `list` = `for-each-ref`, `exists` = `show-ref --exists`, `optimize` = `pack-refs`, and `verify` checks loose ref names/contents and `packed-refs` for consistency.
- **`stripspace`** (`-s|--strip-comments`, `-c|--comment-lines`): git's message cleanup filter for standard input: trims trailing whitespace, collapses and trims empty lines; `-s` drops comment lines, `-c` turns every line into a comment (`core.commentChar`).
- **`column`** (`--mode[=<style>]`, `--raw-mode=<n>`, `--width=<n>`, `--padding=<n>`, `--indent=<string>`, `--nl=<string>`, `--command=<name>`): Lays lines from standard input out in columns (`column`/`row`/`plain`, `dense`), honouring `column.ui` and `column.<name>`.
- **`read-tree`** (`<tree-ish>...`, `--empty`, `--prefix=<dir>/`, `-m`, `--reset`, `-u`, `-i`, `-n`, `--trivial`, `--aggressive`): Reads trees into the index: plain replace, bind under a prefix, one-way merge, two-way merge (branch switch, carrying local index changes) and trivial three-way merge, optionally updating the working tree. Paths that would need conflict stages are refused (rgit's index has none).
- **`update-index`** (`--add`, `--remove`, `--force-remove`, `--replace`, `--cacheinfo <mode>,<object>,<path>`, `--chmod=(+|-)x`, `--refresh`, `-q`, `--ignore-missing`, `--info-only`, `--index-info`, `--stdin`, `-z`, `-g|--again`, `--verbose`, `<file>...`): Low-level index editing with git's in-order option handling. `--assume-unchanged`, `--skip-worktree` and stage numbers are refused (no entry flags or stages in rgit's index).
- **`checkout-index`** (`-a`, `-f`, `-q`, `-n`, `-u`, `--prefix=<string>`, `--temp`, `--stage=<n>|all`, `--stdin`, `-z`, `<file>...`): Copies index entries to the working tree, a prefix directory, or temporary files.
- **`diff-tree`** (`<tree-ish> [<tree-ish>]`, `-r`, `-t`, `--root`, `-m`, `--stdin`, `-s`, `-v`, `--pretty[=<format>]`, `--no-commit-id`, `[<path>...]`), **`diff-index`** (`[--cached] [-m] <tree-ish> [<path>...]`) and **`diff-files`** (`[<path>...]`): The raw plumbing diffs between two trees, a tree and the index or working tree, and the index and the working tree. Output: raw (`:<modes> <hashes> <status> TAB <path>`), `-p`, `--name-only`, `--name-status`, `--stat`, `--shortstat`, `--numstat`, `--summary`, `-z`, `--abbrev[=<n>]`, `-R`, `--quiet`, `--exit-code`. No rename detection or combined diffs.
- **`merge-file`** (`-p`, `-q`, `--diff3`, `--zdiff3`, `--ours`, `--theirs`, `--union`, `--marker-size=<n>`, `--object-id`, `-L <label>`, `<current> <base> <other>`): Three-way merge of files (or blobs), identical to git's xdiff merge output; exit status is the number of conflicts.
- **`mktree`** (`-z`, `--missing`, `--batch`): Builds tree objects from `ls-tree` formatted input. **`mktag`** (`--no-strict`): Validates tag object text from standard input with git's fsck checks and stores it.
- **`unpack-file <blob>`**: Writes a blob to a `.merge_file_XXXXXX` temporary file and prints its name. **`get-tar-commit-id`**: Prints the commit id recorded in a tar archive made by `archive` (which now writes git's pax global header for commits).
- **Packfiles**: objects are read from loose files or from packfiles in `.git/objects/pack` (index v1/v2, offset and ref deltas), so repositories packed by git work; `fsck`, short-hash lookup, `gc`/`prune`, and local `fetch`/`push`/`clone` (including the remote's `packed-refs`) handle packed objects.
- **`repack`** (`-a`, `-A`, `-d`, `--window=<n>`, `--depth=<n>`): Packs loose objects (or with `-a` everything reachable) into a new packfile with delta compression; `-d` removes redundant loose objects and older packs.
- **`pack-objects`** (`--revs`, `--all`, `--unpacked|--incremental`, `--non-empty`, `--window=<n>`, `--depth=<n>`, `--progress`, `--stdout | <base-name>`): Builds a pack from object names or revisions on standard input. **`unpack-objects`** (`-n`, `-r`): Explodes a pack from standard input into loose objects.
- **`index-pack`** (`<pack-file>`, `-o <index>`, `--stdin`, `--fix-thin`, `--keep`, `--verify`): Builds the `.idx` for a pack (byte-identical to git's), or stores a pack read from standard input. **`verify-pack`** (`-v`, `-s`): Checks packs and lists their objects and delta chains. **`show-index`**: Lists a pack index read from standard input.
- **`prune-packed`** (`-n`): Removes loose objects that are also in a pack.
- **`commit-graph`** (`write [--reachable | --stdin-commits | --stdin-packs] [--append]`, `verify`): Writes and checks git's commit-graph file (identical to git's output; not used for rgit's own walks). **`multi-pack-index`** (`write [--preferred-pack=<pack>]`, `verify`, `expire`): Writes and checks git's multi-pack-index (identical to git's output).
- **`fast-export`** (`--all | <rev>...`, `--signed-tags=<mode>`, `--no-data`, `--full-tree`, `--export-marks`, `--import-marks`, `--use-done-feature`, `--mark-tags`, `--reference-excluded-parents`, `--show-original-ids`): Writes history as a `fast-import` stream, matching git's output. **`fast-import`** (`--force`, `--stats`, `--done`, `--date-format`, `--export-marks`, `--import-marks`): Replays such a stream (`blob`, `commit` with `M`/`D`/`C`/`R`/`N`/`deleteall`, `tag`, `reset`, `alias`, ...), producing the same object ids as git.
- **`sparse-checkout`** (`init [--cone|--no-cone]`, `set <dir>...`, `add <dir>...`, `list`, `reapply`, `disable`, `check-rules`): Limits the working tree to selected directories (cone mode) or `.gitignore`-style patterns. Uses git's files (`.git/info/sparse-checkout`, `core.sparseCheckout`, index version 3 skip-worktree bits); `status`, `diff`, `commit -a`, `stash`, `switch` and the plumbing diffs treat left-out files as unchanged, and the patterns are re-applied after commands that rewrite the working tree.
- **`submodule`** (`add [-b <branch>] [--name <name>] <repository> [<path>]`, `status [--cached] [--recursive]`, `init`, `update [--init] [--recursive]`, `deinit [-f] (--all|<path>...)`, `foreach [--recursive] <command>`, `sync`, `set-url`, `set-branch`): Nested repositories recorded as gitlink entries (mode 160000) plus `.gitmodules`. `add <path>` stages a submodule's current commit, `status` reports `(new commits)`, checkouts create the directory, diffs show `Subproject commit <hash>`.
- **Synonyms**: `stage` = `add`, `init-db` = `init`, `fsck-objects` = `fsck`.

---

## Installation & Build

Requires [Rust and Cargo](https://rustup.rs/) (edition 2021+).

```bash
git clone https://github.com/SebiSomu/rgit.git
cd rgit
cargo build --release
```

The compiled binary will be placed at `target/release/rgit` (or `target/release/rgit.exe` on Windows).

---

## Isolated Test Harness (`rtest`)

To safely execute commands without altering the main repository's `.git` folder, `rgit` includes an isolated test harness:

```bash
# Initialize and test in test-sandbox/
cargo run --bin rtest -- init
cargo run --bin rtest -- add .
cargo run --bin rtest -- commit -m "Sandbox commit"

# Test multi-repo workflows (e.g., clone, fetch, pull)
cargo run --bin rtest -- --dir upstream init
cargo run --bin rtest -- clone upstream local
cargo run --bin rtest -- --dir local pull

# Clean up sandbox environment
cargo run --bin rtest -- --clean
```

---

## License

MIT License.
