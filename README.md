# rgit

**A Git implementation written in Rust, built from scratch.**

`rgit` reimplements Git's object model and more than a hundred of its commands. It reads and writes Git's own on-disk formats (loose objects, packfiles, the index, refs, reflogs, config, bundles, commit-graph, multi-pack-index), so a repository created by `rgit` can be opened by real `git` and the other way round.

It started as a way to learn how version control works inside, and it has grown into a tool you can use every day on your own projects.

> **Status.** rgit is checked against Git on a large set of scenarios (see [Testing](#testing)), but it has not been run on very large or unusual repositories. Use it next to Git first, and work on a copy of anything you cannot afford to lose. The [user guide](USER_GUIDE.md) lists the [known differences](USER_GUIDE.md#known-differences-and-limits).

---

## Highlights

- **123 commands**, from everyday porcelain (`add`, `commit`, `branch`, `merge`, `rebase`, `stash`, `bisect`) to plumbing (`cat-file`, `update-index`, `pack-objects`, `commit-graph`).
- **Git-compatible storage.** Zlib objects, SHA-1 ids, index v2/v3, packfiles with offset and ref deltas, `packed-refs`, reflogs, `.git` worktree files.
- **Faithful algorithms.** Myers diff with git's xdiff compaction, rename and copy detection (`diffcore-rename`), three-way merge, `log --graph`, `--follow`, `range-diff`, `blame`, interactive rebase, patch mode (`add -p`).
- **Two remote transports.** Local paths, and GitHub through its REST API with a personal access token.
- **A git that explains itself.** `--explain` and `--teach` narrate what a command did and what to do next (on standard error, so scripts are unaffected). `rgit conflicts` lists unresolved conflicts in plain words, `rgit undo` takes back the last operation (even `reset --hard`) after showing what it will change, `rgit oops` suggests the most likely fix right after a mistake, `rgit wip` saves unfinished work per branch in one word, `rgit fixup` folds a fix into an earlier commit, `rgit absorb` works out which commit each fix belongs to, and `rgit sync` fetches, rebases and pushes with a plan.
- **A safety net.** 25 unit tests and 92 end-to-end scenario snapshots that pin down the output of the commands.

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
| **Everyday work: stage, commit, clean up** | `status`, `add`, `rm`, `mv`, `commit`, `restore`, `clean` |
| **History and inspection** | `log`, `show`, `diff`, `blame`, `annotate`, `whatchanged`, `shortlog`, `grep`, `describe`, `name-rev`, `show-branch`, `last-modified`, `range-diff`, `cherry`, `repo` |
| **Branches, tags and working trees** | `branch`, `switch`, `checkout`, `tag`, `worktree`, `sparse-checkout`, `submodule` |
| **Merging, rebasing and replaying** | `merge`, `conflicts`, `merge-base`, `rebase`, `cherry-pick`, `revert`, `replay`, `rerere`, `banana-split`, `merge-tree`, `merge-file` |
| **Undo, recovery and health** | `undo`, `redo`, `oops`, `wip`, `fixup`, `absorb`, `sync`, `reset`, `stash`, `reflog`, `bisect`, `fsck`, `gc`, `prune` |
| **Remotes and collaboration** | `remote`, `fetch`, `push`, `pull`, `bundle`, `request-pull`, `fmt-merge-msg` |
| **Patches and email** | `apply`, `format-patch`, `am`, `mailsplit`, `mailinfo`, `interpret-trailers`, `check-mailmap`, `patch-id` |
| **Export and import** | `archive`, `get-tar-commit-id`, `fast-export`, `fast-import` |
| **Objects, refs and the index (plumbing)** | `hash-object`, `cat-file`, `write-tree`, `ls-tree`, `commit-tree`, `mktree`, `mktag`, `unpack-file`, `rev-parse`, `rev-list`, `ls-files`, `check-ignore`, `check-ref-format`, `show-ref`, `symbolic-ref`, `update-ref`, `pack-refs`, `for-each-ref`, `refs`, `replace`, `notes`, `update-index`, `checkout-index`, `read-tree`, `diff-tree`, `diff-index`, `diff-files`, `stripspace`, `column` |
| **Storage and maintenance** | `repack`, `pack-objects`, `unpack-objects`, `index-pack`, `verify-pack`, `show-index`, `prune-packed`, `commit-graph`, `multi-pack-index`, `count-objects`, `maintenance`, `for-each-repo`, `hook` |

Synonyms: `stage` = `add`, `init-db` = `init`, `fsck-objects` = `fsck`.

## Remotes

`clone`, `fetch`, `pull` and `push` work with:

1. **A local path**, a folder containing a repository or a bare repository.
2. **GitHub**, as `owner/repo` or an HTTPS URL, using a personal access token (scope `repo`). The token is read from `GITHUB_TOKEN`, `RGIT_GITHUB_TOKEN`, `remote.<name>.token` or `github.token`.

SSH and the plain git smart-HTTP protocol are not supported.

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
├── remote/        clone, fetch, push, pull, bundle, GitHub client
├── storage/       gc, fsck, packfiles, maintenance
├── plumbing/      low-level and utility commands
├── export/        archive, fast-export, fast-import
├── ui/            narration layer
└── bin/rtest.rs   sandbox runner
```

| Module | Files | What it holds |
|---|---|---|
| [`src/core/`](src/core) | 15 | Repository fundamentals every domain builds on: object store, packfiles, index, refs, reflog, commits, config, revision parsing, dates, editor, ref-name rules, wildmatch. Never imports from a domain. |
| [`src/workspace/`](src/workspace) | 63 | Everything that touches the working tree and index: `add`, `rm`, `mv`, `status`, `commit`, `restore`, `reset`, `clean`, `branch`, `switch`, `checkout`, `stash`, `worktree`, `sparse-checkout`, `submodule`, patch mode, the plumbing for index and working tree. |
| [`src/references/`](src/references) | 54 | Refs and everything built on them: `tag`, `notes`, `replace`, `reflog`, `show-ref`, `symbolic-ref`, `update-ref`, `pack-refs`, `for-each-ref`, `refs`, `check-ref-format`. |
| [`src/history/`](src/history) | 50 | Reading history: `log`, `blame`, `grep`, `rev-list`, `shortlog`, `bisect`, `name-rev`, `show-branch`, `range-diff`, `last-modified`, `cherry`, `graph`, `--follow`. |
| [`src/mail/`](src/mail) | 34 | Patches and mail: `apply`, `format-patch`, `am`, `mailsplit`, `mailinfo`, `interpret-trailers`, `mailmap`, `request-pull`, `fmt-merge-msg`. |
| [`src/merge/`](src/merge) | 41 | Combining and rewriting: `merge`, `rebase`, `cherry-pick`, `revert`, `replay`, `rerere`, `merge-tree`, `merge-file`, `banana-split`, the sequencer and conflict handling. |
| [`src/storage/`](src/storage) | 41 | Object storage and maintenance: `gc`, `fsck`, `prune`, `repack`, `pack-objects`, `index-pack`, `verify-pack`, `commit-graph`, `multi-pack-index`, `maintenance`, `count-objects`. |
| [`src/remote/`](src/remote) | 26 | Talking to other repositories: `clone`, `fetch`, `push`, `pull`, `remote`, `bundle`, and the GitHub REST API client. |
| [`src/diff/`](src/diff) | 21 | Diff machinery and commands: Myers diff, hunks, rename/copy detection, combined diffs, `diff`, `diff-tree`, `diff-index`, `diff-files`, `patch-id`. |
| [`src/plumbing/`](src/plumbing) | 38 | Low-level and utility commands: `init`, `hash-object`, `cat-file`, `ls-tree`, `config`, `rev-parse`, `var`, `help`, `hook`, `column`, `stripspace`, `mktree`, `mktag`. |
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

To add a command: write its logic in the command module, its `Args` struct and `dispatch_<command>` function in `<command>/cli.rs`, then add one variant to the `Commands` enum and one dispatch line in `src/cli.rs`.

More detail about conventions (imports, file size, comments, narration) is in `.agents/skills/rgit_agentic_guidelines/SKILL.md`.

## Testing

```bash
cargo test
```

- **Unit tests** (`tests/unit/`) cover parsing and formatting helpers.
- **Scenario tests** (`tests/cli/`) run the real binary in temporary repositories with a fixed identity, fixed dates and relabelled hashes, and compare the full output, exit codes and final repository state with stored [insta](https://insta.rs) snapshots (`tests/cli/scenarios/snapshots/`). They run on Linux and Windows.
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
