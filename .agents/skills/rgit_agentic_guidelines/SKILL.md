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

`rgit` is a modular, lightweight Git implementation in Rust. Code is grouped by domain; inside each domain, thin command modules sit on top of engines, and every domain builds on `core/`:

```text
src/
├── main.rs                   # Entry point calling cli::handle_commands()
├── cli.rs                    # Global flags, the `Commands` enum (one variant per subcommand) and one dispatch line per command
├── ui.rs                     # Narration layer (--explain/--teach/--plain, RGIT_VERBOSITY): before/after snapshots, reports, exit hook
├── ui/
│   ├── before.rs             # Narration printed before a command acts: warnings the user should see first (e.g. pushing a protected branch).
│   ├── narrate.rs            # Narration support: repo snapshots, tree/line counts, last change per path, conflict hunks, risk scan, suggestions
│   ├── report.rs             # Narration after a command: the `report` dispatcher, per-command reports and the shared summaries they use.
│   │   ├── merge.rs          # Reports for merge, rebase, cherry-pick/revert and conflicts, with the next steps to take.
│   │   ├── refs.rs           # Reports for log, tag and branch.
│   │   ├── remote.rs         # Reports for fetch, pull, push and clone.
│   │   └── workspace.rs      # Reports for working-tree commands: add, commit, status, reset, restore, switch, checkout, stash, rm, clean, diff.
│   └── types.rs              # Types: RepoSnapshot, Verbosity
├── core.rs                   # Declares the core modules
├── core/                     # Repository fundamentals every domain builds on (no command entry points)
│   ├── check_ref_format.rs   # Git ref name rules, slash normalization, branch shorthand validation
│   ├── commit.rs             # Commit building, parsing (parse_commit_object, split_commit_message), parent chains, ancestor walks (is_reachable, collect_ancestors)
│   ├── config.rs             # Local & global .gitconfig reader/writer/author identity
│   ├── date.rs               # Civil-date math, timezone offsets, approxidate parsing (shared by log, commit, rev-list, mail)
│   ├── editor.rs             # Editor command resolution (GIT_EDITOR/core.editor/...) and launching
│   ├── eol.rs                # Line-ending conversion (core.autocrlf, core.eol, text/eol attributes) between the working tree and the object store
│   │   └── types.rs          # Types: TextAttr, EolAttr, Action, AutoCrlf, Settings
│   ├── index.rs              # Binary staging index (.git/index) reader and writer
│   ├── object_store.rs       # Object compression, hashing, tree writing, and refs/replace substitution on read
│   ├── pack.rs               # Packfile engine: idx parse/write, packed object lookup (read_object fallback), deltas, pack writer
│   ├── reflog.rs             # Reflog append/parse, <ref>@{n} resolution, entry deletion and expiry
│   ├── refs.rs               # Reference resolution, branch refs, and HEAD state transitions
│   ├── repo.rs               # Repository path resolution (`repo::path(".git/...")`), main vs linked worktree
│   ├── rev_parse.rs          # Revision syntax resolution, worktree root discovery, property queries
│   ├── types.rs              # Types shared by core and every domain (parsed commits, signatures, trees, packs, loose objects, reflog entries)
│   └── wildmatch.rs          # git wildmatch() port (WM_PATHNAME, WM_CASEFOLD)
├── diff.rs                   # Declares the diff modules
├── diff/                     # Diff engines (Myers, xdiff-style hunks, renames, combined diffs, raw/plumbing diffs) and the diff commands
│   ├── combine.rs            # git combine-diff.c port: diff --cc of unmerged paths vs the working tree
│   ├── diff.rs               # diff (porcelain on the raw diff engine: worktree/index/commits, ranges, all formats, renames)
│   │   └── cli.rs            # CLI: diff (clap args + dispatch)
│   ├── diff_files.rs         # diff-files (common diff options)
│   │   └── cli.rs            # CLI: diff-files (clap args + dispatch)
│   ├── diff_index.rs         # diff-index (--cached, -m, common diff options)
│   │   └── cli.rs            # CLI: diff-index (clap args + dispatch)
│   ├── diff_tree.rs          # diff-tree (-r, -t, --root, -m, --stdin, -v/--pretty, common diff options)
│   │   └── cli.rs            # CLI: diff-tree (clap args + dispatch)
│   ├── hunks.rs              # git-compatible line diffs: xdiff change compaction, unified hunks with function context, diffstat, binary literals
│   ├── myers.rs              # Myers diff algorithm & unified diff formatting
│   ├── patch_id.rs           # patch-id (--stable, --unstable, --verbatim)
│   │   ├── cli.rs            # CLI: patch-id (clap args + dispatch)
│   │   ├── engine.rs         # git patch-id port: patch stream splitting, hunk scanning, unstable/stable/verbatim hashing
│   │   └── types.rs          # Types: PatchIdOptions
│   ├── plumbing.rs           # Raw diffs: tree walk, tree/index/worktree comparison, raw/name/stat/patch rendering
│   │   └── types.rs          # Types: DiffPlumbingOptions
│   ├── rename.rs             # git diffcore-rename port: exact/basename/similarity rename+copy detection, -M/-C parsing, limit warnings
│   │   └── types.rs          # Types: RenameOptions
│   └── types.rs              # Diff types shared across domains (DiffOp, DiffSide, DiffChange, DiffStatEntry, FileDiff)
├── export.rs                 # Declares the export modules
├── export/                   # Exporting/importing history: archive, fast-export, fast-import
│   ├── archive.rs            # archive (tar, tar.gz/tgz, zip archives with path filters and prefix)
│   │   ├── cli.rs            # CLI: archive (clap args + dispatch)
│   │   ├── engine.rs         # Archive packing, tar/tar.gz/zip streams, timestamp preservation, path filters
│   │   └── types.rs          # Types: ArchiveFormat
│   ├── fast_export.rs        # fast-export (revs, marks, --no-data, --full-tree, --signed-tags)
│   │   ├── cli.rs            # CLI: fast-export (clap args + dispatch)
│   │   ├── engine.rs         # fast-import stream writer (ref naming, commit order, marks)
│   │   └── types.rs          # Types: FastExportOptions
│   ├── fast_import.rs        # fast-import (--force, --stats, --done, marks)
│   │   ├── cli.rs            # CLI: fast-import (clap args + dispatch)
│   │   ├── engine.rs         # fast-import stream reader (marks, branch trees, commits, tags, refs)
│   │   └── types.rs          # Types: FastImportInput, FastImportState
│   └── get_tar_commit_id.rs  # get-tar-commit-id
│       └── cli.rs            # CLI: get-tar-commit-id (clap args + dispatch)
├── history.rs                # Declares the history modules
├── history/                  # Walking and presenting history: log/show, rev-list, graph, blame, shortlog, name-rev, bisect, grep
│   ├── bisect.rs             # bisect (start, bad, good, skip, reset, log, run)
│   │   ├── cli.rs            # CLI: bisect (clap args + dispatch)
│   │   ├── engine.rs         # Bisect candidate ordering, checkout of the next commit, first-bad / outcome reports
│   │   └── types.rs          # Types: BisectAction, BisectOutcome
│   ├── blame.rs              # blame (line-by-line commit attribution, -L ranges, uncommitted changes), annotate
│   │   ├── cli.rs            # CLI: blame, annotate (clap args + dispatch)
│   │   ├── engine.rs         # Line attribution algorithm, diff-mapping, tree lookups, date formatting
│   │   └── types.rs          # Types: BlameLine
│   ├── changelog.rs          # changelog (Markdown from conventional commits, per version; --prepend)
│   │   ├── cli.rs            # CLI: changelog (clap args + dispatch)
│   │   ├── engine.rs         # Logic of changelog
│   │   └── types.rs          # Types: Entry
│   ├── cherry.rs             # cherry (-v, --abbrev[=<n>], <upstream> [<head> [<limit>]])
│   │   ├── cli.rs            # CLI: cherry (clap args + dispatch)
│   │   ├── engine.rs         # Upstream lookup, non-merge commit walks, git-compatible patch IDs, hash abbreviation
│   │   └── types.rs          # Types: CherryOptions
│   ├── compare.rs            # compare (ahead/behind, commits and files on each side, in-memory merge conflict check)
│   │   ├── cli.rs            # CLI: compare (clap args + dispatch)
│   │   ├── engine.rs         # Logic of compare
│   │   └── types.rs          # Types: Change, FileDiff
│   ├── contributors.rs       # contributors (commits, share, co-authors, lines, first/last day; honors .mailmap)
│   │   ├── cli.rs            # CLI: contributors (clap args + dispatch)
│   │   └── types.rs          # Types: Contributor, SortBy
│   ├── follow.rs             # log --follow: rename/copy source search, similarity scoring, rename-aware diff output
│   │   └── types.rs          # Types: FollowPair
│   ├── graph.rs              # git graph.c port: log --graph rows, column mapping, merge layouts
│   │   └── types.rs          # Types: GitGraph, GraphState
│   ├── grep.rs               # grep (worktree, --cached, and revision search with regex/fixed strings)
│   │   ├── cli.rs            # CLI: grep (clap args + dispatch)
│   │   ├── engine.rs         # Matcher construction, pathspec filtering, content search
│   │   └── types.rs          # Types: GrepFileResult, GrepOptions
│   ├── hotspots.rs           # hotspots (files that change most, have most authors, change together)
│   │   ├── cli.rs            # CLI: hotspots (clap args + dispatch)
│   │   ├── engine.rs         # Logic of hotspots
│   │   └── types.rs          # Types: FileStats, Hotspots
│   ├── last_modified.rs      # last-modified (-r, -t, -z, <revision-range>, paths)
│   │   ├── cli.rs            # CLI: last-modified (clap args + dispatch)
│   │   ├── engine.rs         # Per-path last-change lookup over a range (recursion, trees, boundary commits)
│   │   └── types.rs          # Types: LastModifiedOptions
│   ├── log.rs                # log (pretty formats, --date, --decorate, -p/--stat/--name-status/--raw, notes, --graph, --follow), show (commits, tags, trees, blobs), whatchanged
│   │   ├── cli.rs            # CLI: log, show, whatchanged (clap args + dispatch)
│   │   ├── engine.rs         # log/show arg parsing, pretty formats, date modes, decorations, diff output layout
│   │   └── types.rs          # Types: LogDateMode, LogDecorate, LogFormat, LogOptions
│   ├── name_rev.rs           # name-rev (--name-only, --tags, --refs, --exclude, --all, --annotate-stdin)
│   │   ├── cli.rs            # CLI: name-rev (clap args + dispatch)
│   │   ├── engine.rs         # git name-rev.c port: tip ranking, name propagation, ref filters/shortening, stdin annotation
│   │   └── types.rs          # Types: NameRevOptions, NameRevTip, RevName, RevNames
│   ├── range_diff.rs         # range-diff (--creation-factor, --left-only, --right-only, -s; two ranges, A...B, base tip1 tip2)
│   │   ├── cli.rs            # CLI: range-diff (clap args + dispatch)
│   │   ├── engine.rs         # git range-diff.c port: normalized patches, exact matches, linear assignment, pair output
│   │   └── types.rs          # Types: RangeDiffOptions, RangePatch
│   ├── rev_list.rs           # rev-list (ranges, --not, ref sets, ordering, limits, filters, paths, output formats)
│   │   ├── cli.rs            # CLI: rev-list (clap args + dispatch)
│   │   ├── engine.rs         # Arg parsing, git revision-walk simulation, history simplification, topo sort, filters, select_rev_list_commits
│   │   └── types.rs          # Types: RevListFormat, RevListOptions, RevListRoots, RevWalkState
│   ├── shortlog.rs           # shortlog (-s, -n, -e, --all)
│   │   ├── cli.rs            # CLI: shortlog (clap args + dispatch)
│   │   ├── engine.rs         # Author grouping of reachable commits
│   │   └── types.rs          # Types: ShortlogOptions
│   ├── show_branch.rs        # show-branch (-a, -r, --current, --more, --list, --independent, --merge-base, --topics, --sparse, ...)
│   │   ├── cli.rs            # CLI: show-branch (clap args + dispatch)
│   │   ├── engine.rs         # git show-branch.c port: tip collection, join_revs walk, topo sort, commit naming, column display
│   │   └── types.rs          # Types: ShowBranchOptions
│   ├── stats.rs              # stats (activity, weekdays, authors, languages, biggest commits)
│   │   ├── cli.rs            # CLI: stats (clap args + dispatch)
│   │   ├── engine.rs         # Logic of stats
│   │   └── types.rs          # Types: CommitStat, AuthorStat
│   ├── story.rs              # story (history of a file or directory in plain words, follows renames)
│   │   ├── cli.rs            # CLI: story (clap args + dispatch)
│   │   ├── engine.rs         # Logic of story
│   │   └── types.rs          # Types: Event
│   ├── timeline.rs           # timeline (day-by-day digest: commits, authors, branches, merges, lines)
│   │   ├── cli.rs            # CLI: timeline (clap args + dispatch)
│   │   ├── engine.rs         # Logic of timeline
│   │   └── types.rs          # Types: Event
│   └── types.rs              # History types shared by several history commands
├── mail.rs                   # Declares the mail modules
├── mail/                     # Patch and mail workflows: format-patch, am, apply, mailinfo/mailsplit, trailers, mailmap, request-pull
│   ├── am.rs                 # am (mbox/stdin, -k, --continue, --skip, --abort, --quit, --show-current-patch)
│   │   ├── cli.rs            # CLI: am (clap args + dispatch)
│   │   ├── engine.rs         # Mailbox split, header decoding, author-preserving commits, .git/rebase-apply state
│   │   └── types.rs          # Types: AmOptions, MailPatch
│   ├── apply.rs              # apply (--check, --stat, --numstat, --summary, --cached, --index, -R, -v, -p<n>)
│   │   ├── cli.rs            # CLI: apply (clap args + dispatch)
│   │   ├── engine.rs         # Patch parsing (incl. binary), hunk placement, binary deltas, shared check/write, diffstat
│   │   └── types.rs          # Types: ApplyOptions, BinaryHunk, FilePatch, HunkFailure, HunkLine, ...
│   ├── check_mailmap.rs      # check-mailmap (--stdin, --mailmap-file, --mailmap-blob, <contact>...)
│   │   └── cli.rs            # CLI: check-mailmap (clap args + dispatch)
│   ├── format_patch.rs       # format-patch (-<n>, --root, --stdout, -o, -n/-N, --start-number, --subject-prefix, -k, --signature)
│   │   ├── cli.rs            # CLI: format-patch (clap args + dispatch)
│   │   ├── engine.rs         # Commit selection, mail headers, git-compatible diff/hunks, diffstat, binary literals
│   │   └── types.rs          # Types: FormatPatchOptions
│   ├── interpret_trailers.rs # interpret-trailers (--trailer, --where, --if-exists, --if-missing, --parse, --in-place, ...)
│   │   ├── cli.rs            # CLI: interpret-trailers (clap args + dispatch)
│   │   ├── engine.rs         # git trailer.c port: trailer block detection, trailer.* config, placement rules, commands, in-place
│   │   └── types.rs          # Types: InterpretTrailersOptions, NewTrailer, TrailerArg, TrailerConf, TrailerConfig, ...
│   ├── mailinfo.rs           # mailinfo (-k, -b, -m, -u/-n/--encoding, --[no-]scissors, --no-inbody-headers, --quoted-cr)
│   │   ├── cli.rs            # CLI: mailinfo (clap args + dispatch)
│   │   ├── engine.rs         # git mailinfo.c port: headers, RFC 2047, in-body headers, scissors, MIME/base64/QP/flowed, charsets
│   │   └── types.rs          # Types: MailTransferEncoding, MailinfoOptions, MailinfoOutput, MailinfoState, QuotedCrAction
│   ├── mailmap.rs            # .mailmap / mailmap.file / mailmap.blob parsing, map_user, contact splitting
│   │   └── types.rs          # Types: CheckMailmapOptions, Mailmap, MailmapEntry, MailmapInfo
│   ├── mailsplit.rs          # mailsplit (-o<dir>, -d<prec>, -f<n>, -b, --keep-cr, --mboxrd, mbox/Maildir/stdin)
│   │   ├── cli.rs            # CLI: mailsplit (clap args + dispatch)
│   │   ├── engine.rs         # mbox From-line detection, message splitting, Maildir listing, numbered file output
│   │   └── types.rs          # Types: MailsplitOptions
│   ├── request_pull.rs       # request-pull ([-p] <start> <url> [<end>])
│   │   ├── cli.rs            # CLI: request-pull (clap args + dispatch)
│   │   ├── engine.rs         # ls-remote style ref listing (local/GitHub), matching-ref lookup, range shortlog, tree diffs
│   │   └── types.rs          # Types: RequestPullOptions
│   └── types.rs              # Mail types shared by several mail commands
├── merge.rs                  # Declares the merge modules
├── merge/                    # Merging and replaying commits: merge engines, conflicts, rerere, sequencer, cherry-pick, revert, rebase
│   ├── banana_split.rs       # banana-split (splits commit by path/dir into atomic commits)
│   │   ├── cli.rs            # CLI: banana-split (clap args + dispatch)
│   │   └── engine.rs         # Path grouping helpers for banana-split
│   ├── cherry_pick.rs        # cherry-pick (commit lists/ranges, -x, -s, -n, -m, --ff, --continue, --skip, --abort, --quit)
│   │   ├── cli.rs            # CLI: cherry-pick (clap args + dispatch)
│   │   └── types.rs          # Types: CherryPickOptions
│   ├── conflicts.rs          # conflicts (rgit-only: unresolved files, who changed each side, --explain <file> side by side)
│   │   ├── cli.rs            # CLI: conflicts (clap args + dispatch)
│   │   └── types.rs          # Types: ConflictHunk
│   ├── fmt_merge_msg.rs      # fmt-merge-msg (-m, --into-name, --log[=<n>], --no-log, -F)
│   │   ├── cli.rs            # CLI: fmt-merge-msg (clap args + dispatch)
│   │   ├── engine.rs         # git fmt-merge-msg.c port: FETCH_HEAD lines, title, tag messages, shortlog credits, branch descriptions
│   │   └── types.rs          # Types: FmtMergeMsgOptions, MergeOrigin, MergeSource
│   ├── merge.rs              # merge, merge-base
│   │   ├── cli.rs            # CLI: merge, merge-base (clap args + dispatch)
│   │   ├── engine.rs         # merge bases/commits; merge result → worktree + index stages 1-3; unmerged-index refusals; reset --merge / state cleanup
│   │   └── types.rs          # Types: FastForwardMode, MergeFavor, MergeOptions
│   ├── merge_file.rs         # merge-file (-p, --diff3, --zdiff3, --ours/--theirs/--union, --object-id, -L)
│   │   ├── cli.rs            # CLI: merge-file (clap args + dispatch)
│   │   ├── engine.rs         # merge-file argument parsing
│   │   └── types.rs          # Types: MergeFileArgs, MergeFileOptions
│   ├── merge_tree.rs         # merge-tree (--name-only, --[no-]messages, -z, --merge-base, --allow-unrelated-histories, --explain)
│   │   ├── cli.rs            # CLI: merge-tree (clap args + dispatch)
│   │   ├── engine.rs         # Line-level 3-way file merge, in-memory tree merge, conflict stages/messages, virtual bases
│   │   └── types.rs          # Types: ConflictRegion, FileMergeResult, MergeMessage, MergePathOutcome, MergePathReport, ...
│   ├── move_commit.rs        # move-commit (cherry-pick onto another branch and drop here; -c/--copy; checked in memory first)
│   │   ├── cli.rs            # CLI: move-commit (clap args + dispatch)
│   │   ├── engine.rs         # Logic of move-commit
│   │   └── types.rs          # Types: Replayed
│   ├── rebase.rs             # rebase (--onto, --keep-base, <branch>, --reapply-cherry-picks, -i, --continue, --abort, --skip, --edit-todo)
│   │   ├── cli.rs            # CLI: rebase (clap args + dispatch)
│   │   ├── engine.rs         # Rebase engine: git's commit selection, up-to-date check, todo editor, replay runner
│   │   └── types.rs          # Types: Action, ItemResult, RebaseOptions, TodoItem
│   ├── replay.rs             # replay (--onto | --advance, --contained, ranges)
│   │   ├── cli.rs            # CLI: replay (clap args + dispatch)
│   │   ├── engine.rs         # In-memory commit replay onto a new base, update-ref command output
│   │   └── types.rs          # Types: ReplayOptions
│   ├── rerere.rs             # rerere (record/replay, clear, forget, diff, status, remaining, gc)
│   │   ├── cli.rs            # CLI: rerere (clap args + dispatch)
│   │   └── engine.rs         # Conflict normalization and IDs, rr-cache store, MERGE_RR, resolution replay
│   ├── revert.rs             # revert (--continue, --abort)
│   │   └── cli.rs            # CLI: revert (clap args + dispatch)
│   ├── sequencer.rs          # .git/sequencer state (todo, head, abort-safety, opts) and -x / -s trailers for cherry-pick
│   ├── split.rs              # split (cut one commit into several by hunk, re-parenting the commits above)
│   │   ├── cli.rs            # CLI: split (clap args + dispatch)
│   │   ├── engine.rs         # Logic of split
│   │   └── types.rs          # Types: PathMap, Piece, SplitSource
│   └── types.rs              # Merge types shared by several merge commands (conflict style, ...)
├── plumbing.rs               # Declares the plumbing modules
├── plumbing/                 # Low-level and utility commands: init/hash-object/cat-file/..., mktree, mktag, rev-parse, config, var, help
│   ├── basic.rs              # init, hash-object, cat-file, write-tree, ls-tree
│   │   └── cli.rs            # CLI: hash-object, cat-file, ls-tree (clap args + dispatch)
│   ├── column.rs             # column (--mode, --width, --padding, --indent, --nl)
│   │   ├── cli.rs            # CLI: column (clap args + dispatch)
│   │   ├── engine.rs         # git column.c port: mode parsing, display width, column/row/dense layout
│   │   └── types.rs          # Types: ColumnEnable, ColumnLayout, ColumnOptions
│   ├── config.rs             # config (get, set, unset, list)
│   │   └── cli.rs            # CLI: config (clap args + dispatch)
│   ├── help.rs               # help (-a, <command>)
│   │   ├── cli.rs            # CLI: help (clap args + dispatch)
│   │   ├── engine.rs         # Command list and per-command help lookup (clap)
│   │   └── types.rs          # Types: CommandHelp
│   ├── hook.rs               # hook run (--ignore-missing, --to-stdin)
│   │   ├── cli.rs            # CLI: hook (clap args + dispatch)
│   │   └── engine.rs         # Hook lookup (core.hooksPath, executable check) and execution
│   ├── mktag.rs              # mktag (--no-strict)
│   │   ├── cli.rs            # CLI: mktag (clap args + dispatch)
│   │   └── engine.rs         # Standalone tag fsck (headers, ident) with git's message ids
│   ├── mktree.rs             # mktree (-z, --missing, --batch)
│   │   ├── cli.rs            # CLI: mktree (clap args + dispatch)
│   │   └── engine.rs         # ls-tree line parsing and checks, tree serialization
│   ├── repo_info.rs          # repo (info, structure)
│   │   ├── cli.rs            # CLI: repo (clap args + dispatch)
│   │   └── engine.rs         # repo info keys, repo structure counts, table/key-value output
│   ├── rev_parse.rs          # rev-parse (revision resolution & repository property queries)
│   │   ├── cli.rs            # CLI: rev-parse (clap args + dispatch)
│   │   └── types.rs          # Types: RevParseOptions
│   ├── stripspace.rs         # stripspace (-s, -c)
│   │   ├── cli.rs            # CLI: stripspace (clap args + dispatch)
│   │   └── engine.rs         # Whitespace cleanup on raw bytes, comment-line prefixing
│   ├── tip.rs                # tip/bingo and zen (a git tip of the day, the Zen of rgit)
│   │   ├── cli.rs            # CLI: tip (clap args + dispatch)
│   │   ├── engine.rs         # Logic of tip
│   │   └── types.rs          # Types: Tip
│   ├── unpack_file.rs        # unpack-file <blob>
│   │   └── cli.rs            # CLI: unpack-file (clap args + dispatch)
│   ├── var.rs                # var (-l, <variable>)
│   │   ├── cli.rs            # CLI: var (clap args + dispatch)
│   │   └── engine.rs         # Logical variables (identity, editors, pager, default branch, shell, global config)
│   └── version.rs            # version (--build-options)
│       └── cli.rs            # CLI: version (clap args + dispatch)
├── references.rs             # Declares the references modules
├── references/               # Ref-level commands: for-each-ref, show/update/symbolic-ref, pack-refs, reflog, tags, notes, replace
│   ├── check_ref_format.rs   # check-ref-format (--normalize/--print, --[no-]allow-onelevel, --refspec-pattern, --branch)
│   │   ├── cli.rs            # CLI: check-ref-format (clap args + dispatch)
│   │   └── types.rs          # Types: CheckRefFormatOptions
│   ├── for_each_ref.rs       # for-each-ref (formats/atoms, --sort, --count, filters, quoting, --color, --stdin)
│   │   ├── cli.rs            # CLI: for-each-ref (clap args + dispatch)
│   │   ├── engine.rs         # for-each-ref: ref-filter port (option/atom parsing, ref collection, filters, values, if/align stack, quoting, versioncmp)
│   │   │   ├── atoms.rs      # Format preparation: atom table, atom lookup, and parsing of each atom's arguments (ref-filter.c's `parse_ref_filter_atom`).
│   │   │   ├── color.rs      # git's `color_parse`: color specifications to ANSI escape sequences for `%(color:...)`.
│   │   │   ├── dates.rs      # Date atoms: date modes, timezone math, relative dates, `strftime` formats.
│   │   │   ├── message.rs    # Commit/tag message atoms: subject, body, signature, sanitized subject.
│   │   │   ├── options.rs    # Argument parsing for `for-each-ref`, like git's parse-options: unique prefixes, `--no-` negations, bundled switches.
│   │   │   ├── output.rs     # Output: quoting styles, display width, `%(align)`/`%(if)` stack, and `format_ref_item`.
│   │   │   ├── refs.rs       # Ref reading and collection: loose and packed refs, symref chains, pattern filters, `--points-at`, worktree branches.
│   │   │   ├── sort.rs       # Sorting: `versioncmp` (with versionsort.suffix) and the `--sort` key comparison.
│   │   │   ├── trailers.rs   # `%(trailers)`: trailer block detection, unfolding and formatting (ref-filter's port of trailer.c).
│   │   │   ├── upstream.rs   # Ref name shortening and remote tracking: `:lstrip`/`:rstrip`/`:short`, refspecs, upstream/push refs, ahead/behind.
│   │   │   └── values.rs     # Atom values for one ref: object fields, people, describe output (ref-filter.c's `grab_values`).
│   │   └── types.rs          # Types: ForEachRefExit, ForEachRefOptions, RefAlign, RefArrayItem, RefAtomArg, ...
│   ├── notes.rs              # notes (list, add, append, edit, copy, show, remove, prune, merge, get-ref; --ref)
│   │   ├── cli.rs            # CLI: notes (clap args + dispatch)
│   │   ├── engine.rs         # Notes refs/trees (git fan-out), notes commits, stripspace, notes merge, note lookup for log/show
│   │   └── types.rs          # Types: NotesMergeStrategy, NotesTree
│   ├── pack_refs.rs          # pack-refs (--all, --no-prune, --include, --exclude)
│   │   ├── cli.rs            # CLI: pack-refs (clap args + dispatch)
│   │   ├── engine.rs         # Option parsing, packed-refs read/lock/write (peeled tags, sorted), pattern-filtered packing
│   │   └── types.rs          # Types: PackRefsOptions, PackedRefEntry
│   ├── reflog.rs             # reflog (show, exists, delete, expire)
│   │   ├── cli.rs            # CLI: reflog (clap args + dispatch)
│   │   └── types.rs          # Types: ReflogAction
│   ├── refs_cmd.rs           # refs (list, exists, verify, optimize)
│   │   └── cli.rs            # CLI: refs (clap args + dispatch)
│   ├── refs_verify.rs        # Loose ref name/content checks and packed-refs checks for refs verify
│   ├── replace.rs            # replace (<object> <replacement>, --graft, --edit, -d, -l, --format)
│   │   ├── cli.rs            # CLI: replace (clap args + dispatch)
│   │   ├── engine.rs         # refs/replace listing/creation/deletion, graft commits, object editing
│   │   └── types.rs          # Types: ReplaceMode, ReplaceOptions
│   ├── show_ref.rs           # show-ref (patterns, --head, --heads/--tags, -d, -s/--hash, --abbrev, --verify, --exists, --exclude-existing)
│   │   ├── cli.rs            # CLI: show-ref (clap args + dispatch)
│   │   ├── engine.rs         # show-ref option parsing, pattern matching, ref resolution/existence, output, --exclude-existing
│   │   └── types.rs          # Types: ShowRefOptions
│   ├── symbolic_ref.rs       # symbolic-ref (read, --short, --no-recurse, -q, set with -m, --delete)
│   │   ├── cli.rs            # CLI: symbolic-ref (clap args + dispatch)
│   │   ├── engine.rs         # Option parsing, raw ref reads, symref resolution (SYMREF_MAXDEPTH), creation conflicts, symref writes
│   │   └── types.rs          # Types: ResolvedRef, SymbolicRefOptions
│   ├── tag.rs                # tag (list/filters/-n, lightweight + annotated, -f, -d), describe (--tags/--always/--long/--abbrev/--exact-match)
│   │   ├── cli.rs            # CLI: tag, describe (clap args + dispatch)
│   │   ├── engine.rs         # Tag listing (loose + packed), tag objects, -n lines, packed tag deletion, describe selection
│   │   └── types.rs          # Types: DescribeOptions, TagOptions
│   ├── types.rs              # Ref types shared by several ref commands
│   └── update_ref.rs         # update-ref (<ref> <new> [<old>], -d, --no-deref, --create-reflog, --stdin [-z] transactions)
│       ├── cli.rs            # CLI: update-ref (clap args + dispatch)
│       ├── engine.rs         # Option parsing, object-id lookup, ref transactions (queue, prepare, commit), C-style unquoting
│       └── types.rs          # Types: PreparedRefUpdate, RefUpdate, UpdateRefOptions, UpdateRefsState
├── remote.rs                 # Declares the remote modules
├── remote/                   # Remotes and transports (local paths, GitHub Git Data API, bundles): clone, fetch, pull, push
│   ├── backup.rs             # backup (every ref into a dated bundle in ~/.rgit-backups, newest 10 kept)
│   │   ├── cli.rs            # CLI: backup (clap args + dispatch)
│   │   ├── engine.rs         # Logic of backup
│   │   └── types.rs          # Types: Backup
│   ├── bundle.rs             # bundle (create [-q] [--version], verify [-q], list-heads, unbundle)
│   │   ├── cli.rs            # CLI: bundle (clap args + dispatch)
│   │   ├── engine.rs         # Bundle header read/write, rev/ref resolution, object enumeration, packfile write/read with deltas
│   │   └── types.rs          # Types: BundleAction, BundleHeader
│   ├── clone.rs              # clone (local transport & GitHub Git Data API: init + fetch + checkout + upstream setup)
│   │   ├── cli.rs            # CLI: clone (clap args + dispatch)
│   │   ├── engine.rs         # Source classification, skeleton repo creation, target dir prep/cleanup, worktree checkout
│   │   └── types.rs          # Types: CloneSource
│   ├── fetch.rs              # fetch (local transport & GitHub Git Data API: download objects, update remote-tracking refs)
│   │   ├── cli.rs            # CLI: fetch (clap args + dispatch)
│   │   ├── engine.rs         # Local filesystem fetch transport, remote branch walking, object downloading
│   │   └── types.rs          # Types: FetchRefResult
│   ├── github.rs             # GitHub Git Data API client, blob/tree/commit uploader, remote ref updates
│   │   └── types.rs          # Types: CommitIdentity, GitHubTreeEntry
│   ├── open.rs               # open (web page of a file, branch, commit or pull request; -p prints it)
│   │   ├── cli.rs            # CLI: open (clap args + dispatch)
│   │   ├── engine.rs         # Logic of open
│   │   └── types.rs          # Types: Host, WebRepo, Target
│   ├── pr_summary.rs         # pr-summary (Markdown pull request description: commits, files, merges cleanly?)
│   │   ├── cli.rs            # CLI: pr-summary (clap args + dispatch)
│   │   ├── engine.rs         # Logic of pr-summary
│   │   └── types.rs          # Types: ChangedFile
│   ├── pull.rs               # pull (fetch from remote + merge upstream branch into current branch)
│   │   └── cli.rs            # CLI: pull (clap args + dispatch)
│   ├── push.rs               # push (local transport & GitHub Git Data API: object transfers, ff checks, ref updates)
│   │   ├── cli.rs            # CLI: push (clap args + dispatch)
│   │   ├── engine.rs         # Remote repo resolution, object enumeration, loose object copier, ref updates
│   │   └── types.rs          # Types: PushStatus
│   ├── remote.rs             # remote (list, verbose, add, remove/rm, rename, get-url, set-url, show)
│   │   ├── cli.rs            # CLI: remote (clap args + dispatch)
│   │   ├── engine.rs         # Remote config queries, name validation, ref directory manipulation, tracking branch collection
│   │   └── types.rs          # Types: RemoteAction, RemoteInfo
│   ├── sync.rs               # sync (fetch, then fast-forward/rebase/merge onto the upstream and push, with a plan first)
│   │   ├── cli.rs            # CLI: sync (clap args + dispatch)
│   │   ├── engine.rs         # Logic of sync
│   │   └── types.rs          # Types: Integration, SyncState
│   └── types.rs              # Remote types shared by several transports (GitHub target, ...)
├── storage.rs                # Declares the storage modules
├── storage/                  # Object storage maintenance: packs, pack indexes, commit-graph, multi-pack-index, gc, prune, fsck
│   ├── chunk_file.rs         # Chunk file format shared by commit-graph and multi-pack-index
│   ├── commit_graph.rs       # commit-graph (write, verify)
│   │   ├── cli.rs            # CLI: commit-graph (clap args + dispatch)
│   │   └── engine.rs         # Commit-graph writer and verifier
│   ├── count_objects.rs      # count-objects (-v, -H)
│   │   ├── cli.rs            # CLI: count-objects (clap args + dispatch)
│   │   ├── engine.rs         # Loose/packed object statistics, human-readable sizes
│   │   └── types.rs          # Types: ObjectCountReport
│   ├── doctor.rs             # doctor (objects, refs, identity, config, hooks, stale locks, worktrees, huge files; --fix)
│   │   ├── cli.rs            # CLI: doctor (clap args + dispatch)
│   │   ├── engine.rs         # Logic of doctor
│   │   └── types.rs          # Types: Severity, AutoFix, Finding
│   ├── fsck.rs               # fsck (object integrity, connectivity, dangling/unreachable reports)
│   │   ├── cli.rs            # CLI: fsck (clap args + dispatch)
│   │   ├── engine.rs         # Object integrity checks, connectivity walks, dangling object reports
│   │   └── types.rs          # Types: FsckIssue, FsckNode, FsckReport, FsckSeverity
│   ├── gc.rs                 # gc (reflog expiry + unreachable loose object pruning, --dry-run)
│   │   ├── cli.rs            # CLI: gc (clap args + dispatch)
│   │   ├── engine.rs         # Reachability roots, unreachable loose object detection, pruning
│   │   └── types.rs          # Types: GcReport
│   ├── index_pack.rs         # index-pack (<pack>, -o, --stdin, --fix-thin, --keep, --verify)
│   │   └── cli.rs            # CLI: index-pack (clap args + dispatch)
│   ├── maintenance.rs        # maintenance (run, register, unregister), for-each-repo (--config, --keep-going)
│   │   ├── cli.rs            # CLI: maintenance, for-each-repo (clap args + dispatch)
│   │   └── engine.rs         # Config path lists, running rgit in other repos, maintenance tasks, maintenance.repo registration
│   ├── multi_pack_index.rs   # multi-pack-index (write, verify, expire)
│   │   ├── cli.rs            # CLI: multi-pack-index (clap args + dispatch)
│   │   └── engine.rs         # Multi-pack-index writer, parser and verifier
│   ├── pack_objects.rs       # pack-objects (--revs, --all, --unpacked, --stdout | <base-name>)
│   │   └── cli.rs            # CLI: pack-objects (clap args + dispatch)
│   ├── prune.rs              # prune (-n, -v, --expire, <head>...)
│   │   ├── cli.rs            # CLI: prune (clap args + dispatch)
│   │   ├── engine.rs         # --expire parsing, stale temp/stray file discovery in .git/objects, empty fan-out cleanup
│   │   └── types.rs          # Types: ObjectDirCruft
│   ├── prune_packed.rs       # prune-packed (-n)
│   │   └── cli.rs            # CLI: prune-packed (clap args + dispatch)
│   ├── repack.rs             # repack (-a, -A, -d, --window, --depth)
│   │   └── cli.rs            # CLI: repack (clap args + dispatch)
│   ├── show_index.rs         # show-index
│   │   └── cli.rs            # CLI: show-index (clap args + dispatch)
│   ├── unpack_objects.rs     # unpack-objects (-n, -r)
│   │   └── cli.rs            # CLI: unpack-objects (clap args + dispatch)
│   └── verify_pack.rs        # verify-pack (-v, -s)
│       └── cli.rs            # CLI: verify-pack (clap args + dispatch)
├── workspace.rs              # Declares the workspace modules
├── workspace/                # Working tree and index: add/status/rm/mv, checkout/switch, restore, reset, clean, stash, commit, worktrees, sparse, submodules
│   ├── absorb.rs             # absorb (fold each staged hunk into the commit that last changed those lines)
│   │   ├── cli.rs            # CLI: absorb (clap args + dispatch)
│   │   ├── engine.rs         # Logic of absorb
│   │   └── types.rs          # Types: StagedHunk, Placement
│   ├── branch.rs             # branch (list -a/-r/-v/-vv/patterns/--merged/--contains/--points-at, create -f/-t/--no-track, -d/-D/-r, -m/-M, -c/-C, -u, --unset-upstream, --show-current), switch, checkout
│   │   ├── cli.rs            # CLI: branch, switch, checkout (clap args + dispatch)
│   │   ├── engine.rs         # branch arg parsing, list rendering (-v/-vv tracking), upstream config, branch config section moves
│   │   └── types.rs          # Types: BranchListEntry, BranchOptions
│   ├── check_ignore.rs       # check-ignore (-q, -v, -n, --no-index, --stdin, -z)
│   │   ├── cli.rs            # CLI: check-ignore (clap args + dispatch)
│   │   ├── engine.rs         # Source/line-tracked ignore rules, deciding-rule lookup, path normalization
│   │   └── types.rs          # Types: CheckIgnoreOptions, SourcedIgnoreRule
│   ├── checkout.rs           # Working tree sync, switch safety checks, index resets
│   ├── checkout_index.rs     # checkout-index (-a, -f, -n, -u, --prefix, --temp, --stdin)
│   │   └── cli.rs            # CLI: checkout-index (clap args + dispatch)
│   ├── clean.rs              # clean
│   │   ├── cli.rs            # CLI: clean (clap args + dispatch)
│   │   ├── engine.rs         # Directory walker for untracked/ignored cleanup candidates
│   │   └── types.rs          # Types: CleanMode
│   ├── commit.rs             # commit (-m/-F/-C/-c, -a, --amend, editor, --author/--date, -s), commit-tree
│   │   ├── cli.rs            # CLI: commit-tree, commit (clap args + dispatch)
│   │   └── types.rs          # Types: CommitOptions
│   ├── commit_summary.rs     # `[branch abbrev] subject` summary + diffstat printed after commit / cherry-pick
│   ├── fixup.rs              # fixup (fold the staged changes into an earlier commit and rewrite the commits above)
│   │   ├── cli.rs            # CLI: fixup (clap args + dispatch)
│   │   ├── engine.rs         # Logic of fixup
│   │   └── types.rs          # Types: Candidate
│   ├── gitignore.rs          # Gitignore parsing and pattern matching
│   ├── ignore.rs             # ignore (add patterns to .gitignore or the global file, untrack what they now ignore)
│   │   ├── cli.rs            # CLI: ignore (clap args + dispatch)
│   │   ├── engine.rs         # Logic of ignore
│   │   └── types.rs          # Types: PatternPlan
│   ├── index_plumbing.rs     # Entry vs worktree state, path normalization, add_index_entry rules, refresh, temp files
│   ├── ls_files.rs           # ls-files (-c, -m, -d, -o, -i, --exclude-standard, -s, -t, -z, pathspecs)
│   │   ├── cli.rs            # CLI: ls-files (clap args + dispatch)
│   │   ├── engine.rs         # Worktree-vs-index state, untracked/ignored walker, ls-files pathspec matching
│   │   └── types.rs          # Types: LsFilesOptions, UntrackedPath, WorktreeState
│   ├── oops.rs               # oops (the most likely fixes right after a mistake)
│   │   ├── cli.rs            # CLI: oops (clap args + dispatch)
│   │   ├── engine.rs         # Logic of oops
│   │   └── types.rs          # Types: Fix, OopsContext
│   ├── patch.rs              # patch mode entry points: add -p, reset -p, checkout -p, restore -p, stash -p
│   │   ├── engine.rs         # Interactive hunk engine for -p modes: patch parsing, hunk split/render, applying selected hunks
│   │   └── types.rs          # Types: HunkUse, InteractiveFile, InteractiveHunk, PatchModeSpec, PatchTarget
│   ├── read_tree.rs          # read-tree (--empty, --prefix, -m 1/2/3 trees, --reset, -u, -i, -n)
│   │   ├── cli.rs            # CLI: read-tree (clap args + dispatch)
│   │   ├── engine.rs         # unpack-trees rules: plain read, --prefix, one/two/three-way merges, worktree checks
│   │   └── types.rs          # Types: ReadTreeOptions, ReadTreePlan
│   ├── reset.rs              # reset
│   │   ├── cli.rs            # CLI: reset (clap args + dispatch)
│   │   └── types.rs          # Types: ResetMode
│   ├── restore.rs            # restore (worktree and staged paths)
│   │   ├── cli.rs            # CLI: restore (clap args + dispatch)
│   │   └── types.rs          # Types: WriteoutStage
│   ├── secrets.rs            # secrets (keys, tokens, passwords, .env files in staged changes, files or history)
│   │   ├── cli.rs            # CLI: secrets (clap args + dispatch)
│   │   ├── engine.rs         # Logic of secrets
│   │   └── types.rs          # Types: Scope, Finding
│   ├── sparse_checkout.rs    # sparse-checkout (init, set, add, list, reapply, disable, check-rules)
│   │   ├── cli.rs            # CLI: sparse-checkout (clap args + dispatch)
│   │   └── engine.rs         # Sparse checkout patterns, cone files, apply to worktree + skip-worktree bits, re-apply hook
│   ├── staging.rs            # add, status (long, -s/--short, -b, --porcelain[=v1], -z, -u[no|normal|all]), rm, mv
│   │   └── cli.rs            # CLI: add, status, rm, mv (clap args + dispatch)
│   ├── stash.rs              # stash (push, pop, apply, list, drop, show, clear)
│   │   ├── cli.rs            # CLI: stash (clap args + dispatch)
│   │   └── types.rs          # Types: StashAction
│   ├── state_files.rs        # In-progress operation state files, stash list storage and apply/pop
│   │   └── types.rs          # Types: StashEntry
│   ├── status.rs             # status --short rendering (XY codes, path quoting, -z), untracked dir collapsing, ## branch header
│   │   └── types.rs          # Types: StatusOptions
│   ├── submodule.rs          # submodule (add, status, init, update, deinit, foreach, sync, set-url, set-branch)
│   │   ├── cli.rs            # CLI: submodule (clap args + dispatch)
│   │   ├── engine.rs         # .gitmodules, nested repo HEAD, URL resolution, running rgit in a submodule
│   │   └── types.rs          # Types: SubmoduleInfo
│   ├── types.rs              # Working-tree types shared by several commands (index file state, ...)
│   ├── undo.rs               # undo/redo and the operation log (what the last command changed, how to take it back)
│   │   ├── cli.rs            # CLI: undo (clap args + dispatch)
│   │   ├── engine.rs         # Logic of undo
│   │   └── types.rs          # Types: RepoState, OpKind, Operation, Captured, Change, RestorePlan
│   ├── update_index.rs       # update-index (--add, --remove, --cacheinfo, --chmod, --refresh, --index-info, --stdin)
│   │   ├── cli.rs            # CLI: update-index (clap args + dispatch)
│   │   └── types.rs          # Types: UpdateIndexFlags
│   ├── whereami.rs           # whereami (branch, upstream, changes, operation in progress, what to run next)
│   │   ├── cli.rs            # CLI: whereami (clap args + dispatch)
│   │   ├── engine.rs         # Logic of whereami
│   │   └── types.rs          # Types: Upstream, Situation
│   ├── wip.rs                # wip (save uncommitted work on a hidden per-branch ref; --restore)
│   │   ├── cli.rs            # CLI: wip (clap args + dispatch)
│   │   ├── engine.rs         # Logic of wip
│   │   └── types.rs          # Types: WipEntry
│   └── worktree.rs           # worktree (add, list, remove, move, lock, unlock, prune)
│       ├── cli.rs            # CLI: worktree (clap args + dispatch)
│       ├── engine.rs         # Worktree listing/lookup, admin dirs, checkout into a new worktree, prune/move, list formatting
│       └── types.rs          # Types: BranchCreation, WorktreeAction, WorktreeInfo
└── bin/
    └── rtest.rs              # Isolated test runner executing rgit within test-sandbox/
```

### Module Responsibilities:

1. **[cli.rs](file:///c:/rgit-main/src/cli.rs)**: Declares the global flags (`--explain/--teach/--plain`) and the `Commands` enum. Each variant only carries the subcommand's clap attributes and docs (`#[command(visible_alias = ...)]`, its about text) and wraps the command's argument struct from its `cli` submodule: `Merge(crate::merge::merge::cli::MergeArgs)`. `handle_commands` initializes narration, then dispatches each variant in one line to `<module>::cli::dispatch_<command>`.
2. **Domains** (`diff/`, `history/`, `merge/`, `mail/`, `remote/`, `references/`, `workspace/`, `storage/`, `export/`, `plumbing/`): code is grouped by what it works on, not by layer. Inside a domain:
   - **Command modules** — `<domain>/<command>.rs`, named after the git command (`merge/cherry_pick.rs`). They contain *only* entry points: they orchestrate engines and `core/`, return `anyhow::Result<()>`, and hold no low-level logic.
   - **CLI submodules** — every command module has `<domain>/<command>/cli.rs` (declared first, `pub mod cli;`) holding, per command, its clap argument struct `<Command>Args` (`#[derive(clap::Args, Debug)]`, fields with `#[arg(...)]`) and `pub fn dispatch_<command>(args: <Command>Args) -> anyhow::Result<()>`, which turns the parsed arguments into calls to the command's functions. Describe an `Args` struct with a plain `//` comment: a `///` doc comment becomes the command's help text. Field `///` docs *are* the help text of each option. To add a command: write the logic in the command module, the `Args` struct and `dispatch_` function in its `cli.rs`, then one enum variant and one dispatch line in `src/cli.rs`.
   - Commands that parse their own arguments take `args: Vec<String>` and call `crate::cli::raw_args_after("<name>", args)`, which re-reads the raw process arguments so tokens clap would consume (such as `--`) reach the command unchanged.
   - **Engines** — the lower-level logic. An engine serving one command lives *under* it as `<domain>/<command>/engine.rs` (declared with `pub mod engine;` at the top of the command module). Engines shared within a domain get their own flat file named for what they are (`diff/myers.rs`, `merge/sequencer.rs`, `workspace/checkout.rs`).
   - A domain may use `core/` and other domains' engines; `core/` never uses a domain. Code needed by several domains belongs in `core/`.
   - Domain modules are declared private (`mod diff;`) in `main.rs`; `cli`, `core` and `ui` stay `pub`.
3. **[core/](file:///c:/rgit-main/src/core)**: Repository fundamentals with no command entry points (object store, packs, index, refs, repo paths, config, reflog, commits, revision parsing, dates, editor, ref-name rules, wildmatch).
4. **Types live in `types.rs` modules, next to the code that owns them** (there is no central types file, and types are not mixed into logic files):
   - a type used by one command family goes in that family's `types` submodule: `<domain>/<command>/types.rs` (declared as `pub mod types;` next to `pub mod engine;` in the command module), or `<domain>/<engine>/types.rs` for a flat shared engine (`diff/plumbing/types.rs`);
   - a type shared by several families of one domain goes in `<domain>/types.rs`; a type used across domains lives in its home domain (the family named in it, else the domain with most users);
   - a type that `core/` needs goes in `core/types.rs`; `core/` must never import a type from a domain;
   - a command's clap `Args` struct is its interface, not a shared type: it lives in the command's `cli.rs` (see CLI submodules).
5. **[core/refs.rs](file:///c:/rgit-main/src/core/refs.rs)**: Manages reference resolution (`HEAD`, branch heads under `refs/heads/`). Handles attached/detached HEAD state transitions.
6. **[core/index.rs](file:///c:/rgit-main/src/core/index.rs)**: Reads and writes the staging index (`.git/index`).
7. **[core/repo.rs](file:///c:/rgit-main/src/core/repo.rs)**: Resolves every path inside `.git`. **Never open `.git/...` paths directly**: wrap them in `crate::core::repo::path(".git/...")`, which returns them unchanged in a main worktree and routes them to the shared `.git` (objects, refs, config, logs/refs, STASH_LIST) or to `.git/worktrees/<id>/` (HEAD, index, MERGE_*/REBASE_*/BISECT_* state) inside a linked worktree.
7. **[bin/rtest.rs](file:///c:/rgit-main/src/bin/rtest.rs)**: Test harness that cleans and targets `test-sandbox/` for all manual or agentic testing.

---

## 3. Best Practices for Implementation

- **Error Handling**: Use `anyhow::Result` and add context with `.context()` or `.with_context()` to help debug file system issues.
- **Branch Validation**: All branch creation and renaming must pass through `refs::is_valid_branch_name` to prevent formatting conflicts (following `git-check-ref-format`).
- **Core vs. Domains**: Code in `core/` must not depend on any domain. When a domain engine grows something several domains need (commit parsing, dates, editors...), move it into `core/`.
- **Explicit Imports** (no glob imports of modules, not even inside functions; importing an enum's variants locally, like `use std::cmp::Ordering::*;`, is fine): Import items from the module that defines them (`use crate::core::object_store::read_object;`, `crate::merge::merge::merge(...)`). Never add `pub use x::*` re-exports to module files, and never `#![allow(unused_imports)]` to hide them.
- **Graph Walks**: Use the BFS-based `is_reachable` ancestor walk in `core/commit.rs` rather than assuming linear history (handles branched or merged histories).
- **Nested Directory Handling**: When deleting branch refs, clean up parent directories under `.git/refs/heads/` recursively if they become empty (so nested namespaces like `feature/login` don't leave empty folder structures).
- **File Size**: When a file passes ~1,000 lines, split it into submodules by topic (e.g. `references/for_each_ref/engine/{options,atoms,refs,values,dates,trailers,output,sort}.rs`), keeping helpers private or `pub(super)`.
- **Index Sorting**: The `.git/index` file must have entries sorted alphabetically by path. Use `BTreeMap` or sort vectors before serialization.

### Narration (src/ui.rs, src/ui/)
- rgit's own explanations go through `crate::ui` (`note`, `detail`, `warn`, `danger`) and are written to **stderr** only; never `println!` prose from a command.
- Levels: `quiet` (git-identical) / `normal` / `explain` / `teach`, from `rgit --plain|--explain|--teach`, `RGIT_VERBOSITY`, or `rgit.verbosity`. Default is `normal` only when stderr is a terminal, so piped runs and parity tests are unchanged; set `RGIT_VERBOSITY=quiet` in test scripts to be explicit.
- New narration belongs in the `report_*` functions under `src/ui/report/` (one file per area: `workspace`, `merge`, `remote`, `refs`; they compare the before/after `RepoSnapshot`) and is wired into the `report` dispatcher in `src/ui/report.rs`; warnings shown before a command acts go in `src/ui/before.rs`. Never narrate from command code.
- Never call `std::process::exit` in rgit code: use `crate::ui::exit(code)`, so the narration knows how the command ended (`src/bin/` tools are the exception).
- To stop with an error message, use the shared helpers in `ui.rs` instead of writing a local `die`/`fatal`/`fail`: `crate::ui::die(code, msg)` (prints `msg` + newline to stderr), `crate::ui::die_fatal(msg)` (prints `fatal: msg`, exits 128) and `crate::ui::die_raw(code, msg)` (prints `msg` as-is, for usage texts with their own line breaks). Commands whose errors are returned as values (like `for-each-ref`'s `ForEachRefExit`) keep their own mechanism.
- Git-style conflict `hint:` blocks are wrapped in `if !crate::ui::narration_replaces_hints()`; do the same for new hints the narration already explains.

### Helper Functions vs. Methods:
- **Methods on Types (`impl Type`), declared right after the type in its `types.rs`**: Use methods for pure data transformations that do not perform disk I/O, process spawning, or repository lookups (e.g., `Action::parse`, `Action::name`, `TodoItem::format_line`, `CleanMode::wants`).
- **Standalone Functions in Engines**: Use standalone functions in engine modules (or `core/`) when interacting with the file system, repository metadata, state files, child processes, or multi-step graph algorithms.

### Comment Requirements for Engine Functions:
All functions in engine modules and `core/` must have a comment written above them explaining what they do and for which feature they are needed. The comments must follow this format:
```rust
/// One or more lines explaining what the helper function does.
// Used by `<command_or_caller>`.
pub fn function_name(...) -> anyhow::Result<...> { ... }
```

### Comment Requirements for Objects:
Every struct, enum, and type alias must have descriptive documentation comments directly preceding its declaration containing:
1. **Description of its use case**: A doc-comment (`///`) detailing what data the object models and why it exists.
2. **Used by a file**: A line comment (`// Used by: <file(s)>`) listing the files where the type is used.
3. **Used by command or helper**: A line comment (`// Used by command/helper: <command_or_helper>`) explaining whether it is consumed by a command, a helper, or both.

Example:
```rust
/// An instruction action assigned to an individual commit entry in an interactive rebase todo file.
// Used by: src/merge/rebase/engine.rs, src/merge/rebase.rs
// Used by command/helper: `parse_edited_todo`, `render_todo_for_editing`, `commit_item`, and the `rebase` command
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Pick,
    Reword,
    ...
}
```
