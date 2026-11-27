# rgit

**A Git implementation written in Rust, built from scratch.**

`rgit` reimplements Git's object model and more than a hundred of its commands, and adds more than thirty of its own that git does not have. It reads and writes Git's own on-disk formats (loose objects, packfiles, the index, refs, reflogs, config, bundles, commit-graph, multi-pack-index) and speaks git's own smart-HTTP protocol, so a repository created by `rgit` can be opened by real `git` and the other way round, and rgit can clone, fetch and push to GitHub or any other git server over HTTPS.

It started as a way to learn how version control works inside, and it has grown into a tool you can use every day on your own projects.

> **Status.** rgit is checked against Git on a large set of scenarios (see [Testing](#testing)), but it has not been run on very large or unusual repositories. Use it next to Git first, and work on a copy of anything you cannot afford to lose. The [user guide](USER_GUIDE.md) lists the [known differences](USER_GUIDE.md#known-differences-and-limits).

---

## Highlights

- **152 commands** (`rgit help -a`), from everyday porcelain (`add`, `commit`, `branch`, `merge`, `rebase`, `stash`, `bisect`) to plumbing (`cat-file`, `update-index`, `pack-objects`, `commit-graph`).
- **Git-compatible storage.** Zlib objects, SHA-1 ids, index v2/v3, packfiles with offset and ref deltas, `packed-refs`, reflogs, `.git` worktree files.
- **Faithful algorithms.** Myers diff with git's xdiff compaction, rename and copy detection (`diffcore-rename`), three-way merge, `log --graph`, `--follow`, `range-diff`, `blame`, interactive rebase, patch mode (`add -p`).
- **git's colors.** `status`, `branch`, `log` (decorations and `--graph` lines), `diff` and every patch, `grep`, `show-branch`, `range-diff` and `hint:` lines are colored exactly as git colors them, following `color.ui`, `color.<command>`, `color.<slot>.<name>` and `--color`/`--no-color`.
- **Remote transports.** Local paths, bundles, and any HTTPS git server (GitHub included) over git's own smart-HTTP protocol (v2 with v0 fallback): fetches negotiate so only new objects come over, tags follow as in git, pushes send only what the server lacks, and logins come from git's credential helpers (`rgit credential`) or a GitHub token.
- **A git that explains itself.** `--explain` and `--teach` narrate what a command did and what to do next (on standard error, so scripts are unaffected), and `reset --soft` says where the changes went. `rgit conflicts` lists unresolved conflicts in plain words, `rgit whereami` shows where you stand and what to run next, and `rgit tip` teaches one git trick at a time.
- **Safer history editing.** `rgit undo` takes back the last operation (even `reset --hard`) after showing what it will change, `rgit oops` suggests the most likely fix right after a mistake, `rgit recover` lists every piece of lost work (reset away, amended, dropped stashes) and brings it back, `rgit wip` saves unfinished work per branch, and `rgit squash`, `reword`, `drop`, `fixup`, `absorb`, `split` and `move-commit` rewrite commits without an interactive rebase todo. See [Beyond git](#beyond-git).
- **Insight into a repository.** `story`, `hotspots`, `stats`, `timeline`, `compare` and `contributors` answer the questions people usually write scripts for; `secrets` and `doctor` check a repository before you share it, and `scrub` removes a leaked secret or a big file from all history (with the same result as `git filter-branch`).
- **A safety net.** 95 unit tests, 136 end-to-end scenario snapshots that pin down the output of the commands, and 18 tests that run clone, fetch, push and ls-remote against real `git http-backend` and check the result with `git fsck`.

## Install

You need [Rust and Cargo](https://rustup.rs/) (edition 2024, so a recent stable toolchain).

```bash
git clone https://github.com/SebiSomu/rgit.git
cd rgit
cargo build --release
```

The binary is `target/release/rgit` (`rgit.exe` on Windows). To install it on your `PATH`:

```bash
cargo install --path . --bin rgit
```

Try it next to Git with an alias (`Set-Alias g rgit` in PowerShell, `alias g=rgit` in bash) before you let it stand in for `git`.

## Quick start

```bash
mkdir demo && cd demo
rgit init
rgit config --global user.name "Ada Lovelace"
rgit config --global user.email ada@example.com

echo "hello" > hello.txt
rgit add hello.txt
rgit commit -m "Add hello.txt"

rgit switch -c feature
echo "more" >> hello.txt
rgit commit -a -m "Extend hello.txt"
rgit log --oneline --graph --all

rgit switch main
rgit merge feature
```

Run `rgit help -a` for the list of commands, or `rgit <command> -h` for one command's options. Add `--explain` before any command to have it narrated:

```bash
rgit --explain merge feature
```

## Documentation

**[USER_GUIDE.md](USER_GUIDE.md)** documents every command and flag with a description, syntax and tested examples, plus configuration, environment variables, remotes, recipes and known limits.

## Commands

| Area | Commands |
|---|---|
| **Getting started and configuration** | `init`, `clone`, `config`, `var`, `version`, `help` |
| **Everyday work: stage, commit, clean up** | `status`, `add`, `rm`, `mv`, `commit`, `restore`, `clean`, `ignore` |
| **History and inspection** | `log`, `show`, `diff`, `blame`, `annotate`, `whatchanged`, `shortlog`, `grep`, `describe`, `name-rev`, `show-branch`, `last-modified`, `range-diff`, `cherry`, `repo` |
| **Branches, tags and working trees** | `branch`, `switch`, `checkout`, `tag`, `worktree`, `sparse-checkout`, `submodule` |
| **Merging, rebasing and replaying** | `merge`, `conflicts`, `merge-base`, `rebase`, `cherry-pick`, `revert`, `replay`, `rerere`, `banana-split`, `merge-tree`, `merge-file` |
| **Undo and recovery** | `undo`, `redo`, `oops`, `recover`, `wip`, `squash`, `reword`, `drop`, `fixup`, `absorb`, `split`, `move-commit`, `sync`, `reset`, `stash`, `reflog`, `bisect` |
| **Understanding and learning** | `whereami`, `story`, `hotspots`, `stats`, `timeline`, `compare`, `contributors`, `tip`, `zen` |
| **Safety and health** | `secrets`, `scrub`, `doctor`, `backup`, `fsck`, `gc`, `prune` |
| **Review and release** | `pr-summary`, `open`, `changelog` |
| **Remotes and collaboration** | `remote`, `fetch`, `push`, `pull`, `ls-remote`, `credential`, `credential-store`, `bundle`, `request-pull`, `fmt-merge-msg` |
| **Patches and email** | `apply`, `format-patch`, `am`, `mailsplit`, `mailinfo`, `interpret-trailers`, `check-mailmap`, `patch-id` |
| **Export and import** | `archive`, `get-tar-commit-id`, `fast-export`, `fast-import` |
| **Objects, refs and the index (plumbing)** | `hash-object`, `cat-file`, `write-tree`, `ls-tree`, `commit-tree`, `mktree`, `mktag`, `unpack-file`, `rev-parse`, `rev-list`, `ls-files`, `check-ignore`, `check-ref-format`, `show-ref`, `symbolic-ref`, `update-ref`, `pack-refs`, `for-each-ref`, `refs`, `replace`, `notes`, `update-index`, `checkout-index`, `read-tree`, `diff-tree`, `diff-index`, `diff-files`, `stripspace`, `column` |
| **Storage and maintenance** | `repack`, `pack-objects`, `unpack-objects`, `index-pack`, `verify-pack`, `show-index`, `prune-packed`, `commit-graph`, `multi-pack-index`, `count-objects`, `maintenance`, `for-each-repo`, `hook` |

Synonyms: `stage` = `add`, `init-db` = `init`, `fsck-objects` = `fsck`, `bingo` = `tip`.

## Beyond git

These commands exist only in rgit. All of them are documented, with examples, in the [user guide](USER_GUIDE.md#command-index).

| Command | What it does |
|---|---|
| `undo`, `redo` | Take back the last operation (commit, merge, rebase, `reset --hard`...) after showing what changes; bring it back. |
| `oops` | After a mistake, the most likely fixes, ready to run. |
| `recover` | Every piece of lost work (commits left behind by a reset, amend, rebase or deleted branch; dropped stashes), numbered; `recover <n>` brings one back. |
| `wip` | Save everything uncommitted on a hidden per-branch ref and clean the tree. |
| `fixup`, `absorb` | Fold staged changes into an earlier commit, or each hunk into the commit that last touched those lines. |
| `squash`, `reword`, `drop` | Fold the last N commits into one; change an older commit's message; remove a commit and replay the rest. |
| `split`, `move-commit` | Split a commit by hunk; move commits to another branch without switching. |
| `sync` | Fetch, then fast-forward, rebase or merge onto the upstream and push, with a plan first. |
| `conflicts`, `whereami` | What is unresolved and who changed what; where you stand and what to run next. |
| `story`, `hotspots`, `stats`, `timeline`, `compare`, `contributors` | Read a repository: one file's history, risky files, numbers, a day-by-day digest, how two branches differ, who wrote what. |
| `secrets`, `scrub` | Look for keys and tokens; remove a file or folder from all history, with a backup and a checklist for the rest. |
| `doctor`, `backup`, `ignore` | Check repository health (`--fix` repairs the safe problems), save every ref to a bundle, ignore files and stop tracking them. |
| `pr-summary`, `open`, `changelog` | A pull request description, the web page of a file or branch, a changelog from conventional commits. |
| `tip`, `zen` | A git tip of the day; the ideas rgit is built on. |

## Remotes

`clone`, `fetch`, `pull` and `push` work with:

1. **A local path**, a folder containing a repository or a bare repository (or a bundle file, for `clone` and `fetch`).
2. **Any HTTPS git server**, GitHub included (also as `owner/repo`), over git's smart-HTTP protocol (v2, with v0 fallback). Logins come from git's credential helpers (`credential.helper`); for GitHub, a token in `GITHUB_TOKEN`, `RGIT_GITHUB_TOKEN`, `remote.<name>.token` or `github.token` is used first. `rgit config --global rgit.httpTransport api` switches GitHub back to the older REST API transport.

SSH is not supported.

## Project layout

Code is grouped by what it works on, not by layer. Every domain builds on `core/`; `core/` never imports from a domain.

```text
src/
├── main.rs        entry point (runs the CLI on a large-stack thread)
├── cli.rs         global flags (--explain/--teach/--plain), the Commands enum, one dispatch line per command
├── core/          repository fundamentals
├── workspace/     working tree, index, branches, stash, worktrees
├── references/    refs, tags, notes, reflog
├── history/       log, blame, grep, rev-list, bisect, graph
├── merge/         merge, rebase, cherry-pick, revert, sequencer, conflicts
├── diff/          diff engines and diff commands
├── mail/          patches, mailboxes, trailers
├── remote/        clone, fetch, push, pull, bundle, smart HTTP, credentials
├── storage/       gc, fsck, packfiles, maintenance
├── plumbing/      low-level and utility commands
├── export/        archive, fast-export, fast-import
├── ui/            narration layer
└── bin/rtest.rs   sandbox runner
```

| Module | Files | What it holds |
|---|---|---|
| [`src/core/`](src/core) | 18 | Repository fundamentals every domain builds on: object store, packfiles, index, refs, reflog, commits, config, revision parsing, dates, editor, ref-name rules, wildmatch, git-style colors. Never imports from a domain. |
| [`src/workspace/`](src/workspace) | 109 | Everything that touches the working tree and index: `add`, `rm`, `mv`, `status`, `commit`, `restore`, `reset`, `clean`, `branch`, `switch`, `checkout`, `stash`, `worktree`, `sparse-checkout`, `submodule`, patch mode, `undo`, `oops`, `recover`, `wip`, `fixup`, `absorb`, `squash`, `reword`, `drop`, `whereami`, `ignore`, `secrets`, the plumbing for index and working tree. |
| [`src/references/`](src/references) | 54 | Refs and everything built on them: `tag`, `notes`, `replace`, `reflog`, `show-ref`, `symbolic-ref`, `update-ref`, `pack-refs`, `for-each-ref`, `refs`, `check-ref-format`. |
| [`src/history/`](src/history) | 81 | Reading history: `log`, `blame`, `grep`, `rev-list`, `shortlog`, `bisect`, `name-rev`, `show-branch`, `range-diff`, `last-modified`, `cherry`, `graph`, `--follow`, the analysis commands `story`, `hotspots`, `stats`, `timeline`, `compare`, `contributors`, `changelog`, and `scrub`, which rewrites it. |
| [`src/mail/`](src/mail) | 34 | Patches and mail: `apply`, `format-patch`, `am`, `mailsplit`, `mailinfo`, `interpret-trailers`, `mailmap`, `request-pull`, `fmt-merge-msg`. |
| [`src/merge/`](src/merge) | 49 | Combining and rewriting: `merge`, `rebase`, `cherry-pick`, `revert`, `replay`, `rerere`, `merge-tree`, `merge-file`, `banana-split`, `split`, `move-commit`, the sequencer and conflict handling. |
| [`src/storage/`](src/storage) | 45 | Object storage and maintenance: `gc`, `fsck`, `prune`, `repack`, `pack-objects`, `index-pack`, `verify-pack`, `commit-graph`, `multi-pack-index`, `maintenance`, `count-objects`, `doctor`. |
| [`src/remote/`](src/remote) | 60 | Talking to other repositories: `clone`, `fetch`, `push`, `pull`, `sync`, `remote`, `ls-remote`, `bundle`, `backup`, `open`, `pr-summary`, `credential`; the transports (local paths, bundles, git's smart-HTTP protocol with pkt-lines, negotiation and receive-pack, and the older GitHub REST API client) and the HTTP client with its credential-helper login loop. |
| [`src/diff/`](src/diff) | 21 | Diff machinery and commands: Myers diff, hunks, rename/copy detection, combined diffs, `diff`, `diff-tree`, `diff-index`, `diff-files`, `patch-id`. |
| [`src/plumbing/`](src/plumbing) | 42 | Low-level and utility commands: `init`, `hash-object`, `cat-file`, `ls-tree`, `config`, `rev-parse`, `var`, `help`, `hook`, `column`, `stripspace`, `mktree`, `mktag`, `tip`, `zen`. |
| [`src/export/`](src/export) | 15 | Moving history in and out: `archive`, `fast-export`, `fast-import`, `get-tar-commit-id`. |
| [`src/ui/`](src/ui) | 9 | The narration layer behind `--explain`, `--teach` and `--plain`: snapshots, reports, warnings. |

Inside a domain, every command is a thin module on top of its engine, with its own CLI definition and types next to it:

```text
src/merge/
├── merge.rs              entry points of `merge`
├── merge/
│   ├── cli.rs            clap argument struct (MergeArgs) and dispatch_merge()
│   ├── engine.rs         the merge logic
│   └── types.rs          types that belong to merge
├── sequencer.rs          engine shared by cherry-pick, revert and rebase
└── types.rs              types shared by several merge commands
```

To add a command: write its logic in the command module, its `Args` struct and `dispatch_<command>` function in `<command>/cli.rs`, then add one variant to the `Commands` enum and one dispatch line in `src/cli.rs`, and a line with a one-line description in `src/plumbing/help/catalog.rs` (what `rgit help -a` lists; a unit test fails when a command is missing there).

More detail about conventions (imports, file size, comments, narration) is in `.agents/skills/rgit_agentic_guidelines/SKILL.md`.

## Testing

```bash
cargo test
```

- **Unit tests** (`tests/unit/`) cover parsing and formatting helpers.
- **Scenario tests** (`tests/cli/scenarios/`) run the real binary in temporary repositories with a fixed identity, fixed dates and relabelled hashes, and compare the full output, exit codes and final repository state with stored [insta](https://insta.rs) snapshots (`tests/cli/scenarios/snapshots/`). They run on Linux and Windows.
- **HTTP tests** (`tests/cli/smart_http_*.rs`, `ls_remote_parity.rs`) start a small HTTP server running real `git http-backend` and check rgit's clone, fetch, push, tags and ls-remote against git itself (protocol v2 and v0, logins, server hooks), with `git fsck --strict` on every result. They skip themselves when git isn't installed.
- A change in a snapshot means a change in behaviour. After an intended change, review it with `cargo insta review` (or run with `INSTA_UPDATE=always` and read the diff).
- `tests/clippy-baseline.txt` records the current lint count so new warnings are noticed.

See [`tests/README.md`](tests/README.md) for how the harness works.

### Trying commands safely: `rtest`

To run rgit without touching the repository you are working in, use the sandbox runner. It executes every command inside `test-sandbox/`:

```bash
cargo run --bin rtest -- init
cargo run --bin rtest -- add .
cargo run --bin rtest -- commit -m "Sandbox commit"

# Multi-repository workflows
cargo run --bin rtest -- --dir upstream init
cargo run --bin rtest -- clone upstream local
cargo run --bin rtest -- --dir local pull

# Clean up
cargo run --bin rtest -- --clean
```

Never run `cargo run -- init` in the project root: it would create a repository over the project's own `.git`.

## License

MIT License.
