# rgit User Guide

`rgit` is a Git implementation written in Rust. It reads and writes git's own on-disk format (loose objects, packfiles, the index, refs, reflogs and config), so a repository made by `rgit` can be opened by real `git`, and the other way round. This guide explains every command and every flag rgit accepts, with examples you can copy.

> **Status:** rgit is a learning project that has grown into a daily-usable tool. It has been checked against real git on a large set of scenarios, but not on very large or unusual repositories. Until you trust it, work on a copy of any repository you care about. See [Known differences and limits](#known-differences-and-limits).

## Contents

1. [Install and build](#install-and-build)
2. [Quick start](#quick-start)
3. [How to read the syntax lines](#how-to-read-the-syntax-lines)
4. [Global options and narration](#global-options-and-narration)
5. [Configuration, identity and environment](#configuration-identity-and-environment)
6. [Revision syntax cheat sheet](#revision-syntax-cheat-sheet)
7. [Remotes: local paths and GitHub](#remotes-local-paths-and-github)
8. [Command index](#command-index) and the [command reference](#command-reference)
9. [Everyday recipes](#everyday-recipes)
10. [Known differences and limits](#known-differences-and-limits)
11. [Troubleshooting](#troubleshooting)

---

## Install and build

You need [Rust and Cargo](https://rustup.rs/).

```bash
git clone https://github.com/SebiSomu/rgit.git
cd rgit
cargo build --release
```

The binary is `target/release/rgit` (`rgit.exe` on Windows). To put it on your `PATH`:

```bash
cargo install --path . --bin rgit
```

**Use it next to git, not instead of it, at first.** An alias is the safest way:

```powershell
# PowerShell profile
Set-Alias g rgit
```

```bash
# bash / zsh
alias g=rgit
```

**Trying rgit without touching real work.** The project ships a second binary, `rtest`, that runs rgit inside a throw-away `test-sandbox/` folder:

```bash
cargo run --bin rtest -- init
cargo run --bin rtest -- status
```

Never run `cargo run -- init` in the rgit source folder itself: it would create a repository over the project's own `.git`.

## Quick start

```bash
mkdir demo && cd demo
rgit init                                  # create a repository (branch: main)
rgit config --global user.name "Ada Lovelace"
rgit config --global user.email ada@example.com

echo "hello" > hello.txt
rgit status                                # hello.txt is untracked
rgit add hello.txt                         # stage it
rgit commit -m "Add hello.txt"             # record it

rgit switch -c feature                     # new branch
echo "more" >> hello.txt
rgit commit -a -m "Extend hello.txt"       # -a stages tracked changes first
rgit log --oneline --graph --all           # look at the history

rgit switch main
rgit merge feature                         # bring the work back
rgit branch -d feature                     # tidy up
```

If you skip the identity step, commits are recorded as `rgit <rgit@example.com>`.

## How to read the syntax lines

| Notation | Meaning |
|---|---|
| `<name>` | A value you must supply. |
| `[name]` | Optional. |
| `a \| b` | Choose one. |
| `name...` | May be repeated. |
| `--` | Ends the options; everything after it is a path. |

Short flags can be combined where git allows it (`-fd`, `-sn`). Write options exactly as the command lists them (`--flag=<value>` where an equals sign is shown).

Each command below has a short summary, its syntax, a description, the exact option list printed by `rgit <command> -h` (folded away under *Options*), and examples that were run against the real program.

## Global options and narration

These go **before** the command name:

| Option | Meaning |
|---|---|
| `--explain` | Narrate what the command actually did, and what it means. |
| `--teach` | Like `--explain`, plus the refs and index changes underneath. |
| `--plain` | Exactly git's output; no rgit narration. |
| `-h`, `--help` | Show help. |
| `-V`, `--version` | Show the version. |

```bash
rgit --explain merge feature
rgit --teach reset --hard HEAD~1
rgit --plain status
```

**What narration is.** Every command that changes the repository is watched: rgit takes a snapshot of the state before it runs and compares it afterwards, then tells you in plain words what happened, why nothing happened, or what to do next (conflict reports, warnings before risky actions, next steps). Narration goes to **standard error only**, so standard output is exactly git's output and pipes keep working.

**When it is on.** By default narration is *normal* (one-line notes, conflict reports, warnings) only when standard error is a terminal; in scripts and pipes it is off. You can choose the level permanently:

| Level | How to set it |
|---|---|
| `quiet` (same as `plain`) | `--plain`, `RGIT_VERBOSITY=quiet`, `rgit config rgit.verbosity quiet` |
| `normal` | `RGIT_VERBOSITY=normal`, `rgit config rgit.verbosity normal` |
| `explain` | `--explain`, `RGIT_VERBOSITY=explain`, `rgit config rgit.verbosity explain` |
| `teach` | `--teach`, `RGIT_VERBOSITY=teach`, `rgit config rgit.verbosity teach` |

A command-line flag wins over the environment variable, which wins over the config key. Set `NO_COLOR=1` to turn colors off.

## Configuration, identity and environment

**Where settings live.** Repository settings are in `.git/config`. Your personal settings are in `~/.rgitconfig` (in your `HOME`, or `USERPROFILE` on Windows), written with `rgit config --global`. Local values override global ones. `rgit var GIT_CONFIG_GLOBAL` prints the exact path.

```bash
rgit config user.name "Ada Lovelace"        # this repository only
rgit config --global core.editor "code --wait"
rgit config --list
```

**Identity.** Commits use `user.name` / `user.email`, and `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_AUTHOR_DATE`, `GIT_COMMITTER_NAME`, `GIT_COMMITTER_EMAIL`, `GIT_COMMITTER_DATE` override them for one command. With nothing set, the identity is `rgit <rgit@example.com>`.

**Editor.** When a command needs a message and none was given, rgit opens an editor chosen from, in order: `RGIT_EDITOR`, the `core.editor` setting, `VISUAL`, `EDITOR`. Interactive rebase uses `RGIT_SEQUENCE_EDITOR` first. Lines starting with `#` are stripped from the message.

**Environment variables rgit reads**

| Variable | Effect |
|---|---|
| `RGIT_VERBOSITY` | Narration level (see above). |
| `NO_COLOR` | Disable colored narration. |
| `RGIT_EDITOR`, `RGIT_SEQUENCE_EDITOR` | Editor for messages / for the rebase todo list. |
| `RGIT_PAGER` | Pager command. |
| `GITHUB_TOKEN`, `RGIT_GITHUB_TOKEN` | GitHub access token for push, fetch and clone. |
| `GIT_AUTHOR_*`, `GIT_COMMITTER_*` | Identity and dates for commits. |
| `GIT_NO_REPLACE_OBJECTS` | Ignore `refs/replace/` substitutions. |
| `GIT_NOTES_REF` | Which notes ref `notes` uses. |
| `GIT_TEST_DATE_NOW` | Pretend "now" is this time (for repeatable tests). |
| `RGIT_NESTED` | Set when rgit starts rgit (hooks, `for-each-repo`); keeps narration off. |

**Exit codes.** `0` success; `1` the command ran but reports a negative result (conflicts, "not ignored", "not found", differences with `--exit-code`); `2` the command line is wrong (unknown flag, missing argument); `128` a fatal error such as "not a repository" or "bad revision". Individual commands document their own meaning where it matters (for example `merge-base --is-ancestor`, `cat-file -e`, `check-ignore`).

**Synonyms.** `stage` = `add`, `init-db` = `init`, `fsck-objects` = `fsck`.

## Revision syntax cheat sheet

Anywhere a command takes a commit, you can write:

| Form | Meaning |
|---|---|
| `HEAD`, `main`, `v1.0`, `origin/main` | A branch, tag or remote-tracking branch. |
| `3f2a9c1` | An abbreviated hash (long enough to be unique). |
| `HEAD~2` | Two commits back along first parents. |
| `HEAD^`, `HEAD^2` | First / second parent. |
| `HEAD^{tree}`, `v1^{commit}` | Peel to an object type. |
| `HEAD:path/file` | The file (or folder) at that path in that commit. |
| `main@{1}` | Where `main` pointed one move ago (see `reflog`). |
| `A..B` | Commits in `B` that are not in `A`. |
| `A...B` | Commits in either, but not both. |
| `^X`, `--not` | Exclude `X` and its ancestors. |

## Remotes: local paths and GitHub

rgit has two transports for `clone`, `fetch`, `pull` and `push`:

1. **A local path.** Any folder on disk containing a repository, or a bare repository: `rgit clone ../project my-copy`.
2. **GitHub.** `owner/repo` or an HTTPS URL, through GitHub's REST API with a personal access token (scope `repo`).

The token is looked up in this order: `GITHUB_TOKEN`, `RGIT_GITHUB_TOKEN`, the config key `remote.<name>.token`, the config key `github.token`.

```powershell
$env:GITHUB_TOKEN = "ghp_..."                 # PowerShell, current session
rgit clone SebiSomu/rgit
```

```bash
rgit config --global github.token ghp_xxx    # stored in ~/.rgitconfig
```

General git servers over SSH or plain HTTPS git protocol are not supported. Treat the token like a password: do not commit `.rgitconfig`.

---


## Command index


### 1. Getting started and configuration

| Command | What it does |
|---|---|
| [`init`](#rgit-init) | Create an empty repository in the current folder. |
| [`clone`](#rgit-clone) | Copy a repository from a local path or GitHub and check it out. |
| [`config`](#rgit-config) | Read, set, list and remove configuration values. |
| [`var`](#rgit-var) | Print rgit's logical variables (identity, editor, pager...). |
| [`version`](#rgit-version) | Print the rgit version. |
| [`help`](#rgit-help) | List all commands, or show one command's usage. |

### 2. Everyday work: stage, commit, clean up

| Command | What it does |
|---|---|
| [`status`](#rgit-status) | Show what changed: staged, modified, untracked, conflicted. |
| [`add`](#rgit-add) | Stage file contents for the next commit. |
| [`rm`](#rgit-rm) | Remove files from the working tree and/or the index. |
| [`mv`](#rgit-mv) | Move or rename a file and stage the change. |
| [`commit`](#rgit-commit) | Record the staged changes as a new commit. |
| [`restore`](#rgit-restore) | Restore files in the working tree or the index. |
| [`clean`](#rgit-clean) | Delete untracked (and optionally ignored) files. |

### 3. History and inspection

| Command | What it does |
|---|---|
| [`log`](#rgit-log) | Show commit history. |
| [`show`](#rgit-show) | Show a commit, tag, tree or file. |
| [`diff`](#rgit-diff) | Show changes between commits, the index and the working tree. |
| [`blame`](#rgit-blame) | Show which commit last changed each line of a file. |
| [`annotate`](#rgit-annotate) | `blame` in the older tab-separated format. |
| [`whatchanged`](#rgit-whatchanged) | `log` with the raw list of changed files. |
| [`shortlog`](#rgit-shortlog) | Summarize history by author. |
| [`grep`](#rgit-grep) | Search tracked files, the index or old revisions. |
| [`describe`](#rgit-describe) | Name a commit after the nearest tag. |
| [`name-rev`](#rgit-name-rev) | Name a commit relative to the nearest branch or tag. |
| [`show-branch`](#rgit-show-branch) | Compare branch tips column by column. |
| [`last-modified`](#rgit-last-modified) | Show the commit that last changed each path. |
| [`range-diff`](#rgit-range-diff) | Compare two versions of a series of commits. |
| [`cherry`](#rgit-cherry) | List commits not yet applied upstream. |
| [`repo`](#rgit-repo) | Print repository facts and structure counts. |

### 4. Branches, tags and working trees

| Command | What it does |
|---|---|
| [`branch`](#rgit-branch) | List, create, rename, copy and delete branches. |
| [`switch`](#rgit-switch) | Switch to another branch (or detach HEAD). |
| [`checkout`](#rgit-checkout) | Switch branches, or restore files from a commit or the index. |
| [`tag`](#rgit-tag) | List, create and delete tags. |
| [`worktree`](#rgit-worktree) | Manage extra working trees for the same repository. |
| [`sparse-checkout`](#rgit-sparse-checkout) | Check out only some folders. |
| [`submodule`](#rgit-submodule) | Manage repositories nested inside this one. |

### 5. Merging, rebasing and replaying

| Command | What it does |
|---|---|
| [`merge`](#rgit-merge) | Join another branch into the current one. |
| [`conflicts`](#rgit-conflicts) | Explain unresolved merge conflicts in plain words (rgit only). |
| [`merge-base`](#rgit-merge-base) | Find the best common ancestor of commits. |
| [`rebase`](#rgit-rebase) | Replay commits onto a new base. |
| [`cherry-pick`](#rgit-cherry-pick) | Apply the changes of existing commits. |
| [`revert`](#rgit-revert) | Create a commit that undoes an earlier commit. |
| [`replay`](#rgit-replay) | Replay commits onto a new base without touching the working tree. |
| [`rerere`](#rgit-rerere) | Remember and reuse conflict resolutions. |
| [`banana-split`](#rgit-banana-split) | Split one commit into per-file or per-folder commits (rgit only). |
| [`merge-tree`](#rgit-merge-tree) | Merge two commits in memory and print the result. |
| [`merge-file`](#rgit-merge-file) | Three-way merge of three files. |

### 6. Undo, recovery and health

| Command | What it does |
|---|---|
| [`undo`](#rgit-undo) | Undo the last rgit operation: commit, merge, reset --hard, rebase, ... (rgit only). |
| [`redo`](#rgit-redo) | Bring back the operation most recently undone (rgit only). |
| [`oops`](#rgit-oops) | "I just did something wrong": the most likely fixes, ready to run (rgit only). |
| [`wip`](#rgit-wip) | Save everything uncommitted on a hidden per-branch ref and clean the tree; `--restore` brings it back (rgit only). |
| [`fixup`](#rgit-fixup) | Fold the staged changes into an earlier commit and rewrite the commits on top, in one step (rgit only). |
| [`absorb`](#rgit-absorb) | Fold each staged hunk into the commit that last changed those lines, in one rewrite (rgit only). |
| [`split`](#rgit-split) | Split one commit into several by hunk, picking each new commit's hunks and message (rgit only). |
| [`whereami`](#rgit-whereami) | One screen on where you are and what to run next (rgit only). |
| [`sync`](#rgit-sync) | Fetch, then fast-forward, rebase or merge onto the upstream (and push), with a plan printed first (rgit only). |
| [`reset`](#rgit-reset) | Move HEAD and optionally reset the index and working tree. |
| [`stash`](#rgit-stash) | Set work aside and bring it back later. |
| [`reflog`](#rgit-reflog) | Show where HEAD and branches used to point. |
| [`bisect`](#rgit-bisect) | Binary-search the history for the commit that broke something. |
| [`fsck`](#rgit-fsck) | Check the object database for corruption. |
| [`gc`](#rgit-gc) | Clean up: expire old reflog entries and delete unreachable objects. |
| [`prune`](#rgit-prune) | Delete unreachable loose objects. |

### 7. Remotes and collaboration

| Command | What it does |
|---|---|
| [`remote`](#rgit-remote) | Manage the remotes this repository knows. |
| [`fetch`](#rgit-fetch) | Download new commits from a remote. |
| [`push`](#rgit-push) | Upload commits to a remote. |
| [`pull`](#rgit-pull) | Fetch from a remote and merge it into the current branch. |
| [`bundle`](#rgit-bundle) | Pack history into a single file for offline transfer. |
| [`request-pull`](#rgit-request-pull) | Write a pull-request summary. |
| [`fmt-merge-msg`](#rgit-fmt-merge-msg) | Build a merge commit message from FETCH_HEAD-style lines. |

### 8. Patches and email

| Command | What it does |
|---|---|
| [`apply`](#rgit-apply) | Apply a patch file to the working tree and/or the index. |
| [`format-patch`](#rgit-format-patch) | Export commits as email-style patch files. |
| [`am`](#rgit-am) | Apply a mailbox of patches as commits. |
| [`mailsplit`](#rgit-mailsplit) | Split a mailbox into one file per message. |
| [`mailinfo`](#rgit-mailinfo) | Extract the commit message and patch from one email. |
| [`interpret-trailers`](#rgit-interpret-trailers) | Add or parse trailers such as Signed-off-by. |
| [`check-mailmap`](#rgit-check-mailmap) | Show the canonical name and email from .mailmap. |
| [`patch-id`](#rgit-patch-id) | Compute stable IDs for patches. |

### 9. Export and import

| Command | What it does |
|---|---|
| [`archive`](#rgit-archive) | Export a tree as a tar, tar.gz or zip file. |
| [`get-tar-commit-id`](#rgit-get-tar-commit-id) | Read the commit ID stored in an archive. |
| [`fast-export`](#rgit-fast-export) | Write history as a fast-import stream. |
| [`fast-import`](#rgit-fast-import) | Read a fast-import stream into the repository. |

### 10. Objects, refs and the index (plumbing)

| Command | What it does |
|---|---|
| [`hash-object`](#rgit-hash-object) | Compute (and optionally store) an object's hash. |
| [`cat-file`](#rgit-cat-file) | Show an object's contents, type or size. |
| [`write-tree`](#rgit-write-tree) | Write the index as a tree object. |
| [`ls-tree`](#rgit-ls-tree) | List the contents of a tree. |
| [`commit-tree`](#rgit-commit-tree) | Create a commit object from a tree. |
| [`mktree`](#rgit-mktree) | Build a tree from `ls-tree` formatted text. |
| [`mktag`](#rgit-mktag) | Validate and store a tag object. |
| [`unpack-file`](#rgit-unpack-file) | Write a blob to a temporary file. |
| [`rev-parse`](#rgit-rev-parse) | Resolve revisions and query repository properties. |
| [`rev-list`](#rgit-rev-list) | List commits reachable from given revisions. |
| [`ls-files`](#rgit-ls-files) | List files in the index and working tree. |
| [`check-ignore`](#rgit-check-ignore) | Tell which paths are ignored, and by which rule. |
| [`check-ref-format`](#rgit-check-ref-format) | Check that a reference name is valid. |
| [`show-ref`](#rgit-show-ref) | List references and their commits. |
| [`symbolic-ref`](#rgit-symbolic-ref) | Read, set and delete symbolic references. |
| [`update-ref`](#rgit-update-ref) | Set or delete a reference safely. |
| [`pack-refs`](#rgit-pack-refs) | Move loose references into packed-refs. |
| [`for-each-ref`](#rgit-for-each-ref) | List references with a custom output format. |
| [`refs`](#rgit-refs) | Umbrella command for listing, checking and packing references. |
| [`replace`](#rgit-replace) | Make one object stand in for another. |
| [`notes`](#rgit-notes) | Attach notes to commits. |
| [`update-index`](#rgit-update-index) | Edit the index directly. |
| [`checkout-index`](#rgit-checkout-index) | Copy index entries to the working tree. |
| [`read-tree`](#rgit-read-tree) | Read trees into the index. |
| [`diff-tree`](#rgit-diff-tree) | Raw diff between two trees. |
| [`diff-index`](#rgit-diff-index) | Raw diff between a tree and the index or working tree. |
| [`diff-files`](#rgit-diff-files) | Raw diff between the index and the working tree. |
| [`stripspace`](#rgit-stripspace) | Clean up whitespace in text from standard input. |
| [`column`](#rgit-column) | Arrange lines from standard input in columns. |

### 11. Storage and maintenance

| Command | What it does |
|---|---|
| [`repack`](#rgit-repack) | Pack loose objects into a packfile. |
| [`pack-objects`](#rgit-pack-objects) | Create a packfile from objects or revisions. |
| [`unpack-objects`](#rgit-unpack-objects) | Explode a packfile into loose objects. |
| [`index-pack`](#rgit-index-pack) | Build an index for a packfile. |
| [`verify-pack`](#rgit-verify-pack) | Check packfiles and list their objects. |
| [`show-index`](#rgit-show-index) | Print the contents of a pack index. |
| [`prune-packed`](#rgit-prune-packed) | Remove loose objects that are already in a pack. |
| [`commit-graph`](#rgit-commit-graph) | Write and verify the commit-graph file. |
| [`multi-pack-index`](#rgit-multi-pack-index) | Write and verify the multi-pack-index. |
| [`count-objects`](#rgit-count-objects) | Count loose objects and disk usage. |
| [`maintenance`](#rgit-maintenance) | Run housekeeping tasks. |
| [`for-each-repo`](#rgit-for-each-repo) | Run a command in every repository from a config list. |
| [`hook`](#rgit-hook) | Run a repository hook script by hand. |

---

# Command reference


## 1. Getting started and configuration

Create or copy a repository, set your identity, and ask rgit about itself.


### `rgit init`

Create an empty repository in the current folder.

**Syntax:** `rgit init` (no arguments)

Creates the `.git/` directory layout (objects, refs, `HEAD` on branch `main`) in the current folder. It takes no directory argument: create the folder and `cd` into it first. Alias: `init-db`.

<details>
<summary>Options as printed by <code>rgit init -h</code></summary>

```text
Usage: rgit init

Options:
  -h, --help  Print help
```

</details>

**Examples** (run in an empty folder)

```bash
rgit init
```


### `rgit clone`

Copy a repository from a local path or GitHub and check it out.

**Syntax:** `rgit clone [-b <branch>] [-o <origin>] <repository> [<directory>]`

Initializes target directory, fetches remote objects, configures remote & upstream tracking, and checks out the working tree.

<details>
<summary>Options as printed by <code>rgit clone -h</code></summary>

```text
Usage: rgit clone [OPTIONS] <REPOSITORY> [DIRECTORY]

Arguments:
  <REPOSITORY>  
  [DIRECTORY]   

Options:
  -b, --branch <BRANCH>  
  -o, --origin <ORIGIN>  
  -h, --help             Print help
```

</details>

**Examples** (run in the folder that contains `repo`)

```bash
rgit clone repo my-copy
rgit clone -b feature repo only-feature
```


### `rgit config`

Read, set, list and remove configuration values.

**Syntax:** `rgit config [--global] <key> [<value>] | -l | --unset <key>`

Reads and writes configuration. A key alone prints its value, `<key> <value>` sets it, `-l` lists everything, `--unset` removes a key. Local settings live in `.git/config`; `--global` uses `~/.rgitconfig` (on Windows, in your user folder).

<details>
<summary>Options as printed by <code>rgit config -h</code></summary>

```text
Usage: rgit config [OPTIONS] [KEY] [VALUE]

Arguments:
  [KEY]    
  [VALUE]  

Options:
      --global  
  -l, --list    
      --unset   
  -h, --help    Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit config user.name "Ada Lovelace"
rgit config user.name
rgit config --list
rgit config --unset user.name
```


### `rgit var`

Print rgit's logical variables (identity, editor, pager...).

**Arguments and flags:** `-l`, `<variable>`

Prints rgit's logical variables under git's names: `GIT_AUTHOR_IDENT`, `GIT_COMMITTER_IDENT`, `GIT_EDITOR`, `GIT_SEQUENCE_EDITOR`, `GIT_PAGER`, `GIT_DEFAULT_BRANCH`, `GIT_SHELL_PATH`, `GIT_CONFIG_GLOBAL`; `-l` lists the configuration followed by all of them.

<details>
<summary>Options as printed by <code>rgit var -h</code></summary>

```text
usage: rgit var (-l | <variable>)
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit var -l
rgit var GIT_AUTHOR_IDENT
```


### `rgit version`

Print the rgit version.

**Arguments and flags:** `--build-options`

Prints `rgit version <version>`, optionally with CPU, OS, pointer size and build profile.

<details>
<summary>Options as printed by <code>rgit version -h</code></summary>

```text
usage: rgit version [--build-options]

    --[no-]build-options  also print build options
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit version
rgit version --build-options
```


### `rgit help`

List all commands, or show one command's usage.

**Arguments and flags:** `-a|--all`, `<command>`

Lists every command in columns, or shows one command's usage and options (the command's own `-h` text where it has one, clap's help otherwise).

<details>
<summary>Options as printed by <code>rgit help -h</code></summary>

```text
usage: rgit help [-a | --all] [<command>]
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit help -a
rgit help commit
```


## 2. Everyday work: stage, commit, clean up

The commands you will use dozens of times a day.


### `rgit status`

Show what changed: staged, modified, untracked, conflicted.

**Syntax:** `rgit status [-s] [-b] [--porcelain[=v1]] [-z] [-u[<mode>]]`

Shows the current branch, staged changes, unstaged changes and untracked files, each section in path order. During a merge, cherry-pick or revert it shows git's state line and an `Unmerged paths:` section (`both modified:`, `deleted by us:`...). `-s` gives the short two-letter format, `-b` adds the branch line, `--porcelain` is the stable format for scripts, `-z` separates entries with NUL, `-u` chooses how untracked files are shown. Renames are detected (`renamed:    a -> b`).

<details>
<summary>Options as printed by <code>rgit status -h</code></summary>

```text
Usage: rgit status [OPTIONS]

Options:
  -s, --short                          
  -b, --branch                         
      --porcelain[=<PORCELAIN>]        `--porcelain[=v1]`: the short format, stable for scripts
  -z                                   
  -u, --untracked-files [<UNTRACKED>]  
  -h, --help                           Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo more >> a.txt
echo new > u.txt
rgit status
rgit status -s
rgit status --porcelain
```


### `rgit add`

Stage file contents for the next commit.

**Syntax:** `rgit add [-p] <path>...`

Stages file contents: hashes them into blobs and updates the index (`.git/index`). Use `.` for everything under the current folder. Adding a conflicted path marks it resolved (its stages 1-3 become one entry); adding a deleted tracked path stages the deletion. Alias: `stage`.

**Patch mode (`add -p`).** Walks through each file's changes hunk by hunk, with git's prompts, and stages only the hunks you accept. Answer with `y` (stage), `n` (skip), `q` (quit), `a` (this and all later hunks in the file), `d` (neither this nor later ones), `j`/`J`/`k`/`K` (move between hunks), `g` (go to a hunk by number), `/` (search by regex), `s` (split the hunk into smaller ones), `e` (edit the hunk in your editor) and `?` (help). Answers are read line by line, so you can script it by piping them in: `printf 'y\nn\n' | rgit add -p`.

<details>
<summary>Options as printed by <code>rgit add -h</code></summary>

```text
Usage: rgit add [OPTIONS] [PATHS]...

Arguments:
  [PATHS]...  

Options:
  -p, --patch  
  -h, --help   Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo hello > c.txt
rgit add c.txt
rgit add .
```


### `rgit rm`

Remove files from the working tree and/or the index.

**Arguments and flags:** <files...> `--cached`, `-r`, `-f`

Removes files from index and/or working tree; on a conflicted path it removes all stages without the up-to-date checks, resolving it as deleted.

<details>
<summary>Options as printed by <code>rgit rm -h</code></summary>

```text
Usage: rgit rm [OPTIONS] [FILES]...

Arguments:
  [FILES]...  

Options:
  -f, --force      
      --cached     
  -r, --recursive  
  -h, --help       Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit rm --cached b.txt
rgit rm -f a.txt
```


### `rgit mv`

Move or rename a file and stage the change.

**Arguments and flags:** <source> <destination> `-f`

Moves/renames files, staging the change in the index.

<details>
<summary>Options as printed by <code>rgit mv -h</code></summary>

```text
Usage: rgit mv [OPTIONS] <SOURCE> <DESTINATION>

Arguments:
  <SOURCE>       
  <DESTINATION>  

Options:
  -f, --force  
  -h, --help   Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit mv b.txt c.txt
```


### `rgit commit`

Record the staged changes as a new commit.

**Arguments and flags:** `-m <msg>`..., `-F <file>`, `-C|-c <commit>`, `-a`, `--amend`, `-e|--no-edit`, `--author=<who>`, `--date=<when>`, `--reset-author`, `--allow-empty`, `--allow-empty-message`, `-s`, `--cleanup=strip|whitespace|default`, `-q`

Records the index as a commit. The message comes from `-m` (several become paragraphs), a file, another commit, or the editor (`core.editor`/`$EDITOR`, `#` lines stripped), which opens when none is given and to conclude a merge; `-a` stages tracked changes first, `--amend` replaces the tip keeping its parents and author, and `-s` adds a `Signed-off-by:` trailer. Refuses empty commits and messages unless allowed, and refuses to conclude a merge while conflict markers remain. Concluding a conflicted cherry-pick takes the picked commit's author. Output, reflog entries (`commit:`, `commit (initial|amend|merge):`) and stored commits match git's, including the diffstat and create/delete/rename summary (` Date:` shown when the author comes from another commit or `--date`). Identity and dates honour `GIT_AUTHOR_NAME`/`_EMAIL`/`_DATE` and `GIT_COMMITTER_NAME`/`_EMAIL`/`_DATE` like git; otherwise timestamps are recorded in UTC.

<details>
<summary>Options as printed by <code>rgit commit -h</code></summary>

```text
Usage: rgit commit [OPTIONS]

Options:
  -m, --message <MSG>            
  -F, --file <FILE>              
  -C, --reuse-message <COMMIT>   
  -c, --reedit-message <COMMIT>  
  -a, --all                      
      --amend                    
  -e, --edit                     
      --no-edit                  
      --author <AUTHOR>          
      --date <DATE>              
      --reset-author             
      --allow-empty              
      --allow-empty-message      
  -s, --signoff                  
      --cleanup <MODE>           `strip` (drop `#` lines), `whitespace` or `default` [possible values: strip, whitespace, default]
  -q, --quiet                    
  -h, --help                     Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo more >> a.txt
rgit commit -a -m "Update a.txt"
rgit commit --amend -m "Better message"
```


### `rgit restore`

Restore files in the working tree or the index.

**Arguments and flags:** `--staged`, `--worktree`, `--source`, `--ours`, `--theirs`, `-m|--merge`, `--conflict=merge|diff3|zdiff3`

Restores working tree or index files from the index, a tree or a commit. For a conflicted path, `--ours`/`--theirs` write stage 2/3 (removing the file if that side deleted it) and `-m` recreates the conflict (`merge.conflictStyle`, labels `ours`/`base`/`theirs`); without them it's `error: path 'x' is unmerged`. `--staged` (or `--source` with it) resolves a conflicted path to the source's version. Like git, every path is checked first and nothing changes if one fails.

**Patch mode (`restore -p [-S] [-W] [--source=<tree>]`).** Same prompts as `add -p`; accepted hunks are restored in the index (`-S`), the working tree (`-W`), or both, from the given source.

<details>
<summary>Options as printed by <code>rgit restore -h</code></summary>

```text
Usage: rgit restore [OPTIONS] [FILES]...

Arguments:
  [FILES]...  

Options:
  -p, --patch             
  -S, --staged            
  -W, --worktree          
      --source <SOURCE>   
      --ours              For unmerged paths, take stage 2 (ours) / stage 3 (theirs)
      --theirs            
  -m, --merge             Recreate the conflicted merge of unmerged paths
      --conflict <STYLE>  Like `--merge`, with `merge`, `diff3` or `zdiff3` markers
  -h, --help              Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo oops > a.txt
rgit add a.txt
rgit restore --staged a.txt
rgit restore a.txt
```


### `rgit clean`

Delete untracked (and optionally ignored) files.

**Arguments and flags:** `-f`, `-d`, `-x`, `-X`

Prunes untracked and ignored files with `.gitignore` pattern evaluation.

<details>
<summary>Options as printed by <code>rgit clean -h</code></summary>

```text
Usage: rgit clean [OPTIONS]

Options:
  -n, --dry-run  
  -f, --force    
  -d             
  -x             
  -X             
  -h, --help     Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo junk > junk.txt
rgit clean -f
mkdir tmp && echo x > tmp/y.txt
rgit clean -fd
```


## 3. History and inspection

Read the past: logs, diffs, blame, searches and names for commits.


### `rgit log`

Show commit history.

**Arguments and flags:** `[<revision-range>] [[--] <path>...]`, every `rev-list` walk option (`-n`, `--skip`, `--since`/`--until`, `--author`, `--grep`, `--all`, `--first-parent`, `--merges`/`--no-merges`, `--reverse`, `--topo-order`, path limiting), `--oneline`, `--pretty=oneline|short|medium|full|fuller|raw|format:|tformat:`, `--format=<template>` (incl. `%d`/`%D`), `--date=default|iso|iso-strict|short|raw|unix|rfc|relative`, `--decorate[=short|full|no]`/`--no-decorate`, `-p`, `--stat`, `--shortstat`, `--name-only`, `--name-status`, `-s`

Shows history with output identical to git's (separators, `Merge:` lines, decoration order, stat/patch layout); decorations are on by default on a terminal or via `log.decorate`. `--graph` draws git's ASCII commit graph (merges, octopus merges, collapsing lines; implies `--topo-order`), and `--follow <path>` follows one file across renames and copies with git's similarity scoring (`R`/`C` with percentage in `--name-status`, `old => new` in `--stat`, `rename from`/`to` in `-p`).

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit log
rgit log --oneline
rgit log --graph --all --oneline
rgit log -p -1
rgit log --stat -1
rgit log --author=Test --since=2000-01-01 --oneline
rgit log --follow a.txt
rgit log --format='%h %an %s'
```


### `rgit show`

Show a commit, tag, tree or file.

**Arguments and flags:** `[<object>...]`, `log`'s display options, `-- <path>...`

Shows commits (with their patch; `--stat`/`--name-status` replace it), annotated tags (tagger, date and message, then the tagged object), trees (entry list) and blobs (contents), including `<rev>:<path>` objects, exactly as git prints them.

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit show
rgit show HEAD~1:a.txt
rgit show --stat
```


### `rgit diff`

Show changes between commits, the index and the working tree.

**Arguments and flags:** `[--cached|--staged] [<commit> [<commit>]]`, `A..B`, `A...B`, `[--] <path>...`, `--stat`, `--shortstat`, `--numstat`, `--summary`, `--name-only`, `--name-status`, `--raw`, `-s`, `-z`, `-R`, `--abbrev`, `--exit-code`, `--quiet`, `-1|--base`, `-2|--ours`, `-3|--theirs`, `--cc`

git's porcelain diff on the raw diff engine: working tree vs index, index vs `HEAD`/a commit, working tree vs a commit, or two commits (from their merge base with `...`). Renames are detected by default (see *Rename detection*). During a conflict, a path with both sides shows git's combined diff (`diff --cc`, `@@@` hunks with a column per side) unless a stage is chosen; others show `* Unmerged path <p>`, `U` in raw/`--name-status` and `Unmerged` in `--stat`.

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo more >> a.txt
rgit diff
rgit diff --stat
rgit add a.txt
rgit diff --cached
rgit diff main feature
rgit diff main...feature --name-status
```


### `rgit blame`

Show which commit last changed each line of a file.

**Syntax:** `rgit blame [-L <start>,<end>] [-s] [<rev>] [--] <file>`

Shows, for each line, the commit that last changed it, plus author and date; uncommitted lines show as "Not Committed Yet". `-L` limits the range (`10,20`, `10,+5`, `10,`), `-s` hides author and date, a leading `^` marks lines from the root commit.

<details>
<summary>Options as printed by <code>rgit blame -h</code></summary>

```text
Usage: rgit blame [OPTIONS] <ARGS>...

Arguments:
  <ARGS>...  

Options:
  -L <START,END>  
  -s              
  -h, --help      Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit blame a.txt
rgit blame -L 1,2 a.txt
rgit blame -s a.txt
```


### `rgit annotate`

`blame` in the older tab-separated format.

**Arguments and flags:** `-L <start>,<end>`, `[<rev>] <file>`

`blame` in git's tab-separated `annotate` format.

<details>
<summary>Options as printed by <code>rgit annotate -h</code></summary>

```text
Usage: rgit annotate [OPTIONS] <ARGS>...

Arguments:
  <ARGS>...  

Options:
  -L <START,END>  
  -h, --help      Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit annotate a.txt
```


### `rgit whatchanged`

`log` with the raw list of changed files.

**Arguments and flags:** `log`'s options

`log` with the raw change list (`:<old mode> <new mode> <old hash> <new hash> <status>\t<path>`) after each commit, skipping merges and commits that change nothing. `log` and `show` also accept `--raw` now.

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit whatchanged
```


### `rgit shortlog`

Summarize history by author.

**Arguments and flags:** `-s|--summary`, `-n|--numbered`, `-e|--email`, `--all`, `[<rev>]`

Summarizes history grouped by author, with commit counts and subject lines.

<details>
<summary>Options as printed by <code>rgit shortlog -h</code></summary>

```text
Usage: rgit shortlog [OPTIONS] [REV]

Arguments:
  [REV]  

Options:
  -s, --summary   
  -n, --numbered  
  -e, --email     
      --all       
  -h, --help      Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit shortlog -sn
```


### `rgit grep`

Search tracked files, the index or old revisions.

**Arguments and flags:** `-i`, `-v`, `-n`, `-c`, `-l|-L`, `-F`, `--cached`, `-e <pattern>`, `<pattern> [<rev>...] [-- <path>...]`

Searches tracked files, the index, or historical revisions using regex or fixed-string patterns.

<details>
<summary>Options as printed by <code>rgit grep -h</code></summary>

```text
Usage: rgit grep [OPTIONS] [ARGS]... [-- <PATH_SPECS>...]

Arguments:
  [ARGS]...        
  [PATH_SPECS]...  

Options:
  -i, --ignore-case          
  -v, --invert-match         
  -n, --line-number          
  -c, --count                
  -l, --files-with-matches   
  -L, --files-without-match  
  -F, --fixed-strings        
      --cached               
  -e <PATTERN_FLAG>          
  -h, --help                 Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit grep two
rgit grep -n -i TWO
rgit grep -c two HEAD~1
```


### `rgit describe`

Name a commit after the nearest tag.

**Arguments and flags:** `[--tags] [--always] [--long] [--abbrev=<n>] [--exact-match] [<commit>...]`

Names commits after the nearest tag as `<tag>-<n>-g<hash>` (`<n>` = commits since the tag); annotated tags only unless `--tags`, preferring annotated and newer tags on the same commit, with git's error messages (including the "try --tags" hint).

<details>
<summary>Options as printed by <code>rgit describe -h</code></summary>

```text
Usage: rgit describe [OPTIONS] [COMMITS]...

Arguments:
  [COMMITS]...  

Options:
      --tags         
      --always       
      --long         
      --abbrev=<N>   
      --exact-match  
  -h, --help         Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit tag -a v1 -m "first release"
rgit describe
rgit describe --tags --long
```


### `rgit name-rev`

Name a commit relative to the nearest branch or tag.

**Arguments and flags:** `--name-only`, `--tags`, `--refs=<pattern>`, `--exclude=<pattern>`, `--all`, `--annotate-stdin`, `--no-undefined`, `--always`, `<commit>...`

Names revisions relative to the nearest ref (`main~3`, `tags/v1^0`, `main~3^2~1`) with git's rules (tags first, older tags first, first-parent paths preferred); can list every reachable commit or annotate hashes in text read from standard input.

<details>
<summary>Options as printed by <code>rgit name-rev -h</code></summary>

```text
usage: rgit name-rev [<options>] <commit>...
   or: rgit name-rev [<options>] --all
   or: rgit name-rev [<options>] --annotate-stdin

    --[no-]name-only      print only ref-based names (no object names)
    --[no-]tags           only use tags to name the commits
    --[no-]refs <pattern> only use refs matching <pattern>
    --[no-]exclude <pattern>
                          ignore refs matching <pattern>

    --[no-]all            list all commits reachable from all refs
    --[no-]annotate-stdin annotate text from stdin
    --[no-]undefined      allow to print `undefined` names (default)
    --[no-]always         show abbreviated commit object as fallback
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit name-rev HEAD
rgit name-rev --name-only HEAD~1
```


### `rgit show-branch`

Compare branch tips column by column.

**Arguments and flags:** `-a`, `-r`, `--current`, `--more=<n>`, `--list`, `--independent`, `--merge-base`, `--topo-order`, `--date-order`, `--sha1-name`, `--no-name`, `--topics`, `--sparse`, `<rev>|<glob>...`

Shows branch tips and, column by column, which of them contain each commit down to their common ancestor.

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit show-branch main feature
```


### `rgit last-modified`

Show the commit that last changed each path.

**Arguments and flags:** `-r`, `-t`, `-z`, `[<revision-range>] [-- <path>...]`

For each path, the commit that last changed it (git 2.52's command; output ordered by path).

<details>
<summary>Options as printed by <code>rgit last-modified -h</code></summary>

```text
usage: rgit last-modified [--recursive] [--show-trees] [-z] [<revision-range>] [[--] <path>...]

    -r, --recursive       recurse into subtrees
    -t, --show-trees      show tree entries when recursing into subtrees
    -z                    lines are separated by NUL character
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit last-modified
```


### `rgit range-diff`

Compare two versions of a series of commits.

**Arguments and flags:** `--creation-factor=<n>`, `--left-only`, `--right-only`, `-s`; `<range1> <range2>`, `<rev1>...<rev2>`, `<base> <rev1> <rev2>`

Compares two versions of a commit range, pairing up commits (`=`, `!`, `<`, `>`) and showing how the changed patches differ.

<details>
<summary>Options as printed by <code>rgit range-diff -h</code></summary>

```text
usage: rgit range-diff [<options>] <old-base>..<old-tip> <new-base>..<new-tip>
   or: rgit range-diff [<options>] <old-tip>...<new-tip>
   or: rgit range-diff [<options>] <base> <old-tip> <new-tip>

    --creation-factor <n>
                          percentage by which creation is weighted
    --no-dual-color       use simple diff colors
    --left-only           only emit output related to the first range
    --right-only          only emit output related to the second range
    -s, --no-patch        do not show the patch comparison
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit range-diff main..feature main..feature
```


### `rgit cherry`

List commits not yet applied upstream.

**Arguments and flags:** `-v`, `--abbrev[=<n>]`, `[<upstream> [<head> [<limit>]]]`

Lists the commits in `<head>` (default `HEAD`) missing from `<upstream>` (default the branch's upstream), oldest first, marking each `-` if an equivalent change already exists upstream (same patch ID as git computes it, so cherry-picks and whitespace-only differences are recognized) or `+` if it doesn't; merges are ignored and `<limit>` cuts off older commits.

<details>
<summary>Options as printed by <code>rgit cherry -h</code></summary>

```text
Usage: rgit cherry [OPTIONS] [UPSTREAM] [HEAD] [LIMIT]

Arguments:
  [UPSTREAM]  
  [HEAD]      
  [LIMIT]     

Options:
  -v, --verbose       
      --abbrev[=<N>]  
  -h, --help          Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit cherry -v main feature
```


### `rgit repo`

Print repository facts and structure counts.

**Arguments and flags:** `info [--format=keyvalue|nul] [--all | <key>...]`, `structure [--format=table|keyvalue|nul]`

Prints repository facts (`layout.bare`, `layout.shallow`, `object.format`, `references.format`) and counts of refs and reachable objects (git 2.52's command).

<details>
<summary>Options as printed by <code>rgit repo -h</code></summary>

```text
usage: rgit repo info [--format=(keyvalue|nul) | -z] [--all | <key>...]
   or: rgit repo structure [--format=(table|keyvalue|nul) | -z]
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit repo info
rgit repo structure
```


## 4. Branches, tags and working trees

Move around the history and keep several lines of work apart.


### `rgit branch`

List, create, rename, copy and delete branches.

**Arguments and flags:** [<name> [<start-point>]] | -a | -r | -v | -d/-D | -m/-M [--push[=<remote>]] | -c/-C | -u <upstream> | --show-current | --merged/--no-merged/--contains/--no-contains/--points-at <commit>

Without arguments, lists local branches (`*` marks the current one). With a name, creates a branch at `HEAD` or at `<start-point>`. `-a` also lists remote-tracking branches, `-r` lists only those, `-v` adds hash and subject (`-vv` adds upstream). `-d` deletes a fully merged branch, `-D` deletes regardless, `-m`/`-M` rename, `-c`/`-C` copy, `-u` sets the upstream, `--show-current` prints the current branch name. Filters (`--merged`, `--contains`, `--points-at`...) narrow the list. Refuses to delete or rename a branch checked out in another worktree.

**Rename on the remote too (`-m --push[=<remote>]`, rgit only).** `rgit branch -m <old> <new> --push` renames the branch here (reflog and `branch.*` config included), pushes `<new>` to the branch's remote (`branch.<old>.remote`, or the one named with `--push=<remote>`), makes `<remote>/<new>` its upstream and deletes the old name there (the upstream's name, usually `<old>`; skipped when rgit knows of no such remote branch). The plan is printed first. The new name is pushed before the old one is deleted, so the remote never loses the branch; if a remote step fails, the local rename stays and the commands still to run are printed (exit 1). `rgit undo` renames it back locally only. Without `--push`, `-m` behaves exactly like git's.

<details>
<summary>Options as printed by <code>rgit branch -h</code></summary>

```text
error: unknown switch `h'
usage: rgit branch [<options>] [-r | -a] [--merged] [--no-merged]
   or: rgit branch [<options>] [-f] <branch-name> [<start-point>]
   or: rgit branch [<options>] [-l] [<pattern>...]
   or: rgit branch [<options>] [-r] (-d | -D) <branch-name>...
   or: rgit branch [<options>] (-m | -M) [<old-branch>] <new-branch>
   or: rgit branch [<options>] (-c | -C) [<old-branch>] <new-branch>
   or: rgit branch [<options>] [-r | -a] [--points-at]

    -v, --verbose         show hash and subject, give twice for upstream branch
    -q, --quiet           suppress informational messages
    -t, --track           set branch tracking configuration
    --no-track            do not set up tracking
    -u, --set-upstream-to <upstream>
                          change the upstream info
    --unset-upstream      unset the upstream info
    -r, --remotes         act on remote-tracking branches
    --contains <commit>   print only branches that contain the commit
    --no-contains <commit>
                          print only branches that don't contain the commit
    -a, --all             list both remote-tracking and local branches
    -d, --delete          delete fully merged branch
    -D                    delete branch (even if not merged)
    -m, --move            move/rename a branch and its reflog
    -M                    move/rename a branch, even if target exists
    -c, --copy            copy a branch and its reflog
    -C                    copy a branch, even if target exists
    -l, --list            list branch names
    --show-current        show current branch name
    -f, --force           force creation, move/rename, deletion
    --merged <commit>     print only branches that are merged
    --no-merged <commit>  print only branches that are not merged
    --points-at <object>  print only branches of the object
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit branch
rgit branch topic
rgit branch -m topic topic2
rgit branch -a
rgit branch -vv
rgit branch -d topic2
```


### `rgit switch`

Switch to another branch (or detach HEAD).

**Syntax:** `rgit switch [-c <new> [<start-point>]] [--detach] [-f] <branch>`

Switches to a branch. `-c` creates it first, at `<start-point>` (any revision) or the current commit. `--detach` detaches `HEAD` at any revision: a branch, a tag, `HEAD~1`, `main^2`, an abbreviated hash, `origin/main`. A commit without `--detach` is refused like git does (`a branch is expected, got commit`). `rgit switch -` goes back to where you were before the last switch, read from HEAD's reflog like git's `@{-1}`. A name only a remote has (`switch feature` with just `origin/feature`) creates a local branch tracking it, as git guesses. Every switch writes git's `checkout: moving from <a> to <b>` reflog entry. `-f` discards local changes. Refuses in the middle of a merge, rebase, cherry-pick, revert or bisect, and on an unmerged index unless `-f`.

<details>
<summary>Options as printed by <code>rgit switch -h</code></summary>

```text
Usage: rgit switch [OPTIONS] <BRANCH> [START_POINT]

Arguments:
  <BRANCH>       
  [START_POINT]  With `-c`: the commit (any revision) the new branch starts at; default HEAD

Options:
  -c, --create  
      --detach  
  -f, --force   
  -h, --help    Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit switch feature
rgit switch -c new-branch
rgit switch --detach $(rgit rev-parse HEAD~1)
```


### `rgit checkout`

Switch branches, or restore files from a commit or the index.

**Arguments and flags:** `<branch|commit>`, `-b`, `--detach`, `-f`; `[<tree-ish>] [--ours|--theirs|-m|--conflict=<style>] -- <paths>`

Classic branch, commit, and revision switching interface. A local branch name switches to that branch. Anything else rgit's revision parser understands (a tag, lightweight or annotated, `HEAD~1`, `main^`, an abbreviated hash, `origin/main`, `@{1}`) is checked out on a detached `HEAD`. `checkout -` returns to where you were before. `checkout HEAD` stays put. A name only one remote has becomes a new branch tracking it (`branch 'x' set up to track 'origin/x'.`). `-b <new> [<start-point>]` creates a branch at any revision. With paths after `--` it restores them instead: from a tree-ish into index and working tree (resolving conflicts), or from the index, where a conflicted path needs `--ours`/`--theirs` (an error if that side lacks it) or `-m`.

**Patch mode (`checkout -p [<tree-ish>]`).** Same prompts as `add -p`; each accepted hunk is discarded from the working tree (restored from the index or the given tree-ish).

<details>
<summary>Options as printed by <code>rgit checkout -h</code></summary>

```text
Usage: rgit checkout [OPTIONS] [TARGET] [-- <PATHS>...]

Arguments:
  [TARGET]    
  [PATHS]...  Paths after `--`: restore them instead of switching branches

Options:
  -b <CREATE_BRANCH>      
      --detach            
  -f, --force             
  -p, --patch             
      --ours              For unmerged paths, take stage 2 (ours) / stage 3 (theirs)
      --theirs            
  -m, --merge             Recreate the conflicted merge of unmerged paths
      --conflict <STYLE>  Like `--merge`, with `merge`, `diff3` or `zdiff3` markers
  -h, --help              Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit checkout feature
rgit checkout -b other
rgit checkout HEAD~1 -- a.txt
```


### `rgit tag`

List, create and delete tags.

**Arguments and flags:** `[-l] [-n[<num>]] [--contains <commit>] [--points-at <object>] [<pattern>...]`, `[-a] [-f] [-m <msg>... | -F <file>] [-e] <name> [<object>]`, `-d <name>...`

Lists tags (glob patterns, message lines, containment and points-at filters), creates lightweight or annotated tags on any object (tag objects with tagger and message, editor when `-a` has no message, `#` lines stripped), replaces with `-f` (`Updated tag … (was …)`), and deletes loose or packed tags (`Deleted tag … (was …)`), with git's messages and exit codes.

<details>
<summary>Options as printed by <code>rgit tag -h</code></summary>

```text
Usage: rgit tag [OPTIONS] [ARGS]...

Arguments:
  [ARGS]...  

Options:
  -l, --list                
  -d, --delete              
  -a, --annotate            
  -f, --force               
  -m, --message <MSG>       
  -F, --file <FILE>         
  -e, --edit                
  -n [<NUM>]                -n[<num>]: lines of each tag message to print (attached number, like git)
      --contains <COMMIT>   
      --points-at <OBJECT>  
  -h, --help                Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit tag v1.0
rgit tag -a v1.1 -m "Release 1.1"
rgit tag -l
rgit tag -d v1.0
```


### `rgit worktree`

Manage extra working trees for the same repository.

**Arguments and flags:** `add [-b|-B <branch>] [-d] [-f] [--lock [--reason]] <path> [<commit-ish>]`, `list [--porcelain|-v]`, `remove [-f [-f]]`, `move`, `lock [--reason]`, `unlock`, `prune [-n] [-v]`

Manages multiple working trees attached to one repository using git's on-disk layout (`<path>/.git` file → `.git/worktrees/<id>/`), so real git can use them too. Each worktree has its own HEAD, index and in-progress state; objects, refs, config and stashes are shared. A branch can be checked out in only one worktree at a time (`switch`, `checkout` and `branch -d` respect this), `branch -m` updates other worktrees' HEADs, and `gc`/`fsck` treat every worktree's HEAD, index and state files as roots.

<details>
<summary>Options as printed by <code>rgit worktree -h</code></summary>

```text
Subcommands of `worktree`, which manages extra working trees attached to one repository. Each linked worktree has its own HEAD, index and in-progress state under `.git/worktrees/<id>/`, while objects, refs and config stay shared

Usage: rgit worktree <COMMAND>

Commands:
  add     Create a worktree at <path> and check out <commit-ish> in it
  list    List the main worktree and every linked worktree
  remove  Delete a linked worktree (refuses a dirty one unless forced)
  move    Move a linked worktree to a new location
  lock    Protect a linked worktree from being pruned, moved or removed
  unlock  Undo `worktree lock`
  prune   Delete the records of worktrees whose directories no longer exist

Options:
  -h, --help  Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit worktree add ../wt feature
rgit worktree list
rgit worktree remove ../wt
```


### `rgit sparse-checkout`

Check out only some folders.

**Arguments and flags:** `init [--cone|--no-cone]`, `set <dir>...`, `add <dir>...`, `list`, `reapply`, `disable`, `check-rules`

Limits the working tree to selected directories (cone mode) or `.gitignore`-style patterns. Uses git's files (`.git/info/sparse-checkout`, `core.sparseCheckout`, index version 3 skip-worktree bits); `status`, `diff`, `commit -a`, `stash`, `switch` and the plumbing diffs treat left-out files as unchanged, and the patterns are re-applied after commands that rewrite the working tree.

<details>
<summary>Options as printed by <code>rgit sparse-checkout -h</code></summary>

```text
usage: rgit sparse-checkout (init | list | set | add | reapply | disable | check-rules) [<options>]

   init [--cone | --no-cone]
   set [--cone | --no-cone] [--stdin | <patterns>...]
   add [--stdin | <patterns>...]
   list
   reapply [--cone | --no-cone]
   disable
   check-rules [-z] [--cone | --no-cone] [--rules-file <file>]
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit sparse-checkout set d
rgit sparse-checkout list
rgit sparse-checkout disable
```


### `rgit submodule`

Manage repositories nested inside this one.

**Arguments and flags:** `add [-b <branch>] [--name <name>] <repository> [<path>]`, `status [--cached] [--recursive]`, `init`, `update [--init] [--recursive]`, `deinit [-f] (--all|<path>...)`, `foreach [--recursive] <command>`, `sync`, `set-url`, `set-branch`

Nested repositories recorded as gitlink entries (mode 160000) plus `.gitmodules`. `add <path>` stages a submodule's current commit, `status` reports `(new commits)`, checkouts create the directory, diffs show `Subproject commit <hash>`.

<details>
<summary>Options as printed by <code>rgit submodule -h</code></summary>

```text
fatal: not a git repository (or any of the parent directories): .git
```

</details>


## 5. Merging, rebasing and replaying

Combine lines of work and rewrite them.


### `rgit merge`

Join another branch into the current one.

**Arguments and flags:** `<commit>`, `--ff|--no-ff|--ff-only`, `--squash`, `--[no-]commit`, `-m <msg>`..., `-F <file>`, `-s ort|recursive|ours`, `-X ours|theirs`, `-n|--no-stat`, `-q`, `--allow-unrelated-histories`, `--abort`, `--continue`, `--quit`

Fast-forwards when possible (`--no-ff` always records a merge commit, `--ff-only` refuses anything else with git's hints), otherwise runs the same line-level three-way merge as `merge-tree` (non-overlapping edits merge cleanly; mode changes merge separately from content; criss-cross histories use a virtual base). `-X ours`/`-X theirs` resolve content conflicts toward one side; `-s ours` records a merge that keeps `HEAD`'s tree. A clean merge is committed (message from `-m`/`-F`, else git's `Merge branch 'x'[ into <branch>]` per `merge.suppressDest`) and followed by git's diffstat and create/delete-mode summary; works on a detached `HEAD` too. `--no-commit` stops before committing; `--squash` stages the merged result, writes `SQUASH_MSG` (git's "Squashed commit of the following:" log) and records no merge, so the next `rgit commit` makes an ordinary commit. Refuses to start on an unmerged index or while a merge or cherry-pick is in progress. On conflicts it prints git's messages, writes conflict markers into the working tree, stages everything that merged cleanly, records each conflicted path's stages 1 (base), 2 (ours) and 3 (theirs) in the index exactly as git does, writes `MERGE_HEAD`/`MERGE_MSG`/`MERGE_MODE`/`AUTO_MERGE` (and `ORIG_HEAD`), and exits 1; `rgit commit` or `merge --continue` concludes the merge once every conflicted path is resolved, `--abort` restores `HEAD`'s version of every path the merge touched (git's `reset --merge`), and `--quit` forgets the merge but keeps the index and working tree. Not supported: octopus merges and the `resolve`/`octopus`/`subtree` strategies; `-X` options other than `ours`/`theirs`; `-s recursive` runs the ort merge (as git 2.50+ does).

<details>
<summary>Options as printed by <code>rgit merge -h</code></summary>

```text
Usage: rgit merge [OPTIONS] [COMMITS]...

Arguments:
  [COMMITS]...  

Options:
      --ff                         
      --no-ff                      
      --ff-only                    
      --squash                     
      --no-squash                  
      --commit                     
      --no-commit                  
  -m, --message <MESSAGE>          
  -F, --file <FILE>                
  -s, --strategy <STRATEGY>        
  -X, --strategy-option <OPTION>   
      --abort                      
      --continue                   
      --quit                       
  -n, --no-stat                    
      --stat                       
  -q, --quiet                      
      --no-edit                    Accepted for compatibility: rgit never opens an editor for a merge commit
      --allow-unrelated-histories  
  -h, --help                       Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit merge --no-ff feature -m "Merge feature"
```


### `rgit conflicts`

Explain unresolved merge conflicts in plain words (rgit only).

**Syntax:** `rgit conflicts [--explain <file>]`

An rgit-only command. During a merge, rebase, cherry-pick or revert, `conflicts` lists every unresolved file with how many places conflict, which lines, who changed what on each side, and what to do next. `conflicts --explain <file>` shows each conflict in that file side by side (yours, theirs, and the original when `merge.conflictStyle` is `diff3`), with a suggestion. With nothing unresolved it prints `No conflicts: nothing is unresolved.`

<details>
<summary>Options as printed by <code>rgit conflicts -h</code></summary>

```text
No conflicts: nothing is unresolved.
```

</details>


### `rgit merge-base`

Find the best common ancestor of commits.

**Arguments and flags:** `<commit> <commit>...`, `-a|--all`, `--is-ancestor`

Finds the best common ancestor(s) of commits, or checks ancestry via exit code.

<details>
<summary>Options as printed by <code>rgit merge-base -h</code></summary>

```text
Usage: rgit merge-base [OPTIONS] <COMMITS>...

Arguments:
  <COMMITS>...  

Options:
  -a, --all          
      --is-ancestor  
      --octopus      
  -h, --help         Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit merge-base main feature
rgit merge-base --is-ancestor HEAD~1 HEAD
```


### `rgit rebase`

Replay commits onto a new base.

**Arguments and flags:** `[--onto <newbase> | --keep-base] [<upstream> [<branch>]]`, `--[no-]reapply-cherry-picks`, `-i`, `--continue`, `--abort`, `--skip`, `--edit-todo`

Replays the branch's commits that `<upstream>` lacks (git's `<upstream>...HEAD` right side: merges left out, oldest first in topological order) onto `--onto <newbase>`, the merge base with `--keep-base`, or `<upstream>` itself. `<branch>` is switched to first; without `<upstream>` the branch's configured upstream is used (git's "There is no tracking information" help otherwise). Commits whose patch upstream already has are skipped with git's warning unless `--reapply-cherry-picks` (implied by `--keep-base`); commits that start empty are kept and ones that become empty are dropped. When the branch already sits on the new base nothing is rewritten (`Current branch <b> is up to date.` / `HEAD is up to date.`); a pick whose parent is already `HEAD` is fast-forwarded, the rest are replayed with `merge`'s line-level merge (labels `HEAD` and `<abbrev> (<subject>)`) keeping the original author and message. Progress (`Rebasing (i/n)`), the final `Successfully rebased and updated refs/heads/<b>.` / `detached HEAD.`, `ORIG_HEAD`, conflict stops with git's messages and index stages, and the summary printed by `--continue` match git 2.43. `-i` adds `pick`, `reword`, `edit`, `squash`, `fixup` and `drop`; `--continue` refuses while paths are unmerged.

<details>
<summary>Options as printed by <code>rgit rebase -h</code></summary>

```text
Usage: rgit rebase [OPTIONS] [UPSTREAM] [BRANCH]

Arguments:
  [UPSTREAM]  
  [BRANCH]    

Options:
      --onto <NEWBASE>           
      --keep-base                
      --reapply-cherry-picks     
      --no-reapply-cherry-picks  
  -i, --interactive              
      --continue                 
      --abort                    
      --skip                     
      --edit-todo                
  -h, --help                     Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit switch feature
rgit rebase main
```


### `rgit cherry-pick`

Apply the changes of existing commits.

**Arguments and flags:** `<commit>...`, ranges `A..B` / `^X`, `-x`, `-s`, `-n|--no-commit`, `-m <parent-number>`, `--ff`, `--allow-empty`, `--allow-empty-message`, `--keep-redundant-commits`, `--continue`, `--skip`, `--abort`, `--quit`

A port of git's sequencer for picks. Commits are picked in the order given, or with a range walked oldest first like `rev-list --reverse`; each is replayed onto `HEAD` with `merge`'s line-level merge (labels `HEAD` and `<abbrev> (<subject>)`; `-m <n>` diffs a merge against its n-th parent) and committed with the original author and message plus git's summary (`[<branch> <abbrev>] <subject>`, ` Date:`, diffstat, create/delete/rename summary). `-x` appends `(cherry picked from commit <hash>)` and `-s` a `Signed-off-by:` trailer, each after a blank line unless the message already ends in trailers; `-n` only updates the index and working tree (merging into whatever the index holds); `--ff` fast-forwards when `HEAD` is the commit's parent. A pick whose result changes nothing stops with git's status and advice (unless `--keep-redundant-commits`). On conflicts the index holds stages 1-3 and `CHERRY_PICK_HEAD` / `MERGE_MSG` / `AUTO_MERGE` are written; with several commits the rest of the sequence waits in `.git/sequencer` (`todo`, `head`, `abort-safety`, `opts`). `--continue` commits the resolved pick (`commit --no-edit --cleanup=strip`, original author) and goes on, `--skip` drops it (`reset --merge`), `--abort` rewinds to where the sequence started (unless `HEAD` was moved meanwhile), and `--quit` forgets the sequence and keeps everything; `rgit commit` or `rgit reset` concluding the last pick ends the sequence too. Errors, hints, state files and exit codes match git 2.43.

<details>
<summary>Options as printed by <code>rgit cherry-pick -h</code></summary>

```text
Usage: rgit cherry-pick [OPTIONS] [COMMITS]...

Arguments:
  [COMMITS]...  

Options:
  -n, --no-commit                 
      --commit                    
  -x                              
  -s, --signoff                   
  -m, --mainline <PARENT-NUMBER>  
      --ff                        
      --allow-empty               
      --allow-empty-message       
      --keep-redundant-commits    
      --continue                  
      --skip                      
      --abort                     
      --quit                      
  -h, --help                      Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit cherry-pick -x feature
```


### `rgit revert`

Create a commit that undoes an earlier commit.

**Syntax:** `rgit revert <commit> [-n|--no-commit] [--no-edit] | --continue | --abort`

Creates a new commit that undoes `<commit>`, with the same merge and conflict handling as `cherry-pick`. `--no-edit` is accepted for compatibility with git: rgit never opens an editor for the revert message. `-n` applies the inverse to the index and working tree without committing.

<details>
<summary>Options as printed by <code>rgit revert -h</code></summary>

```text
Usage: rgit revert [OPTIONS] [COMMIT]

Arguments:
  [COMMIT]  

Options:
  -n, --no-commit  
      --continue   
      --abort      
      --no-edit    
  -h, --help       Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit revert --no-edit HEAD
```


### `rgit replay`

Replay commits onto a new base without touching the working tree.

**Arguments and flags:** `--onto <newbase>` | `--advance <branch>`, `--contained`, `<revision-range>...`

Re-creates commits on a new base in the object store only, without touching the working tree, index or refs, and prints `update <ref> <new> <old>` lines for `update-ref --stdin` (git 2.44's command). Exits 1 without output when a commit conflicts.

<details>
<summary>Options as printed by <code>rgit replay -h</code></summary>

```text
usage: rgit replay ([--contained] --onto <newbase> | --advance <branch>) <revision-range>...

    --advance <branch>    make replay advance given branch
    --onto <revision>     replay onto given commit
    --contained           advance all branches contained in revision-range
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit replay --onto main main..feature
```


### `rgit rerere`

Remember and reuse conflict resolutions.

**Arguments and flags:** `clear`, `forget <path>...`, `diff`, `status`, `remaining`, `gc`

Records how conflicts were resolved in `.git/rr-cache` (git's format) and applies the recorded resolution when the same conflict shows up again. Run it by hand after a conflict and again after resolving: rgit's `merge`, `rebase` and `commit` don't call it, and conflicts are detected by their markers.

<details>
<summary>Options as printed by <code>rgit rerere -h</code></summary>

```text
usage: rgit rerere [clear | forget <pathspec>... | diff | status | remaining | gc]
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit rerere status
```


### `rgit banana-split`

Split one commit into per-file or per-folder commits (rgit only).

**Arguments and flags:** <commit> `--by-dir`, `-n|--dry-run`

Custom tool that splits a commit into atomic per-path or per-directory commits.

<details>
<summary>Options as printed by <code>rgit banana-split -h</code></summary>

```text
Usage: rgit banana-split [OPTIONS] [COMMIT]

Arguments:
  [COMMIT]  

Options:
      --by-dir   
  -n, --dry-run  
  -h, --help     Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit banana-split --dry-run HEAD~1
```


### `rgit merge-tree`

Merge two commits in memory and print the result.

**Arguments and flags:** `[--write-tree]`, `--name-only`, `--[no-]messages`, `-z`, `--merge-base=<tree-ish>`, `--allow-unrelated-histories`, `--explain`, `<branch1> <branch2>`

Merges two commits entirely in memory and writes only the merged tree (conflicted files contain markers), leaving refs, index and working tree untouched. Default output matches git's: tree hash, conflicted stages (`<mode> <oid> <stage>	<path>`), and messages (`Auto-merging`, `CONFLICT (content|add/add|modify/delete|file/directory)`, binary warnings); exits 0 if clean, 1 on conflicts. `--explain` prints a readable report instead: the ours/theirs/base commits (including virtual bases), a per-path table of what each side did and what the merge decided, and a base/ours/theirs preview of every conflicting region. Renames are not detected.

<details>
<summary>Options as printed by <code>rgit merge-tree -h</code></summary>

```text
Usage: rgit merge-tree [OPTIONS] <BRANCH1> <BRANCH2>

Arguments:
  <BRANCH1>  
  <BRANCH2>  

Options:
      --write-tree                 
      --name-only                  
      --messages                   
      --no-messages                
  -z                               
      --merge-base <TREE-ISH>      
      --allow-unrelated-histories  
      --explain                    
  -h, --help                       Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit merge-tree main feature
rgit merge-tree --name-only main feature
```


### `rgit merge-file`

Three-way merge of three files.

**Arguments and flags:** `-p`, `-q`, `--diff3`, `--zdiff3`, `--ours`, `--theirs`, `--union`, `--marker-size=<n>`, `--object-id`, `-L <label>`, `<current> <base> <other>`

Three-way merge of files (or blobs), identical to git's xdiff merge output; exit status is the number of conflicts.

<details>
<summary>Options as printed by <code>rgit merge-file -h</code></summary>

```text
usage: rgit merge-file [<options>] [-L <name1> [-L <orig> [-L <name2>]]] <file1> <orig-file> <file2>

    -p, --[no-]stdout     send results to standard output
    --[no-]object-id      use object IDs instead of filenames
    --[no-]diff3          use a diff3 based merge
    --[no-]zdiff3         use a zealous diff3 based merge
    --[no-]ours           for conflicts, use our version
    --[no-]theirs         for conflicts, use their version
    --[no-]union          for conflicts, use a union version
    --[no-]marker-size <n>
                          for conflicts, use this marker size
    -q, --[no-]quiet      do not warn about conflicts
    -L <name>             set labels for file1/orig-file/file2
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
printf 'a\nb\n' > base.txt
cp base.txt ours.txt
cp base.txt theirs.txt
echo c >> ours.txt
rgit merge-file -p ours.txt base.txt theirs.txt
```


## 6. Undo, recovery and health

Go back, set work aside, find a bug, and check the repository is sound.


### `rgit undo`

Undo the last rgit operation: commit, merge, reset --hard, rebase, ... (rgit only).

**Syntax:** `rgit undo [--list | -n|--dry-run] [-f|--force]`

An rgit-only command, inspired by Jujutsu's operation log. Every rgit command that changes the repository is recorded automatically in `.git/rgit/oplog` with what it changed: where `HEAD` points, local branches and tags, the index, the stash list, in-progress operation files (`MERGE_HEAD`, `rebase-merge/`, `sequencer/`, `BISECT_*`, ...) and, for commands that can rewrite files (`reset`, `checkout`, `switch`, `restore`, `merge`, `pull`, `rebase`, `cherry-pick`, `revert`, `stash`, `clean`, `rm`, `mv`, `am`, `apply`, ...), a snapshot of the working tree, including uncommitted edits and untracked files. `undo` first prints which operation it is undoing and every change it will make, then puts back exactly those things and nothing else. So undoing `reset --hard` brings back the uncommitted work it discarded, undoing `branch -D` recreates the branch, undoing a conflicted `merge` cancels it, and undoing `commit` moves the branch back while keeping the changes staged.

Before touching anything, `undo` checks that what the operation changed still looks the way the operation left it. If something was changed again since (a file edited, a branch moved by real git), it stops with exit code 1, lists what changed, and suggests `rgit stash` first or `--force`, which overwrites those changes. `-n`/`--dry-run` only prints the preview. `--list` shows the operations `undo` can take back (newest first) and those `redo` can bring back. Repeating `undo` steps further back. Running any new command after an undo forgets what could be redone, as in an editor.

Not recorded: read-only commands; `config`; `push`/`fetch` and remote-tracking refs (they mirror the remote, so undo never moves them and never un-pushes anything); `init`/`clone`; and work done with real git or rgit versions older than this command. Untracked files over 64 MiB and ignored files are not snapshotted. Saved data lives in its own store under `.git/rgit/objects`, so `fsck`, `count-objects` and `gc` see exactly the objects real git would. `gc` treats commits the log refers to like reflog entries. The log keeps the last 1000 operations.

<details>
<summary>Options as printed by <code>rgit undo -h</code></summary>

```text
Undo the last rgit operation (commit, merge, reset --hard, rebase, ...); `--list` shows the log

Usage: rgit undo [OPTIONS]

Options:
      --list     List the recorded operations instead of undoing one
  -n, --dry-run  Show what would be undone without changing anything
  -f, --force    Undo even if something the operation touched has changed since (overwrites it)
  -h, --help     Print help
```

</details>

**Example:**

```text
$ rgit reset --hard HEAD~2
HEAD is now at 1d64c7b first commit
$ rgit undo
Undoing #6: rgit reset --hard HEAD~2  (just now)
  - files restored: a.txt, b.txt, src/lib.txt
  - index (staging area): put back as it was
  - branch 'main': 1d64c7b -> 8055fcc "third commit"
Done. `rgit redo` brings it back.
```


### `rgit redo`

Bring back the operation most recently undone (rgit only).

**Syntax:** `rgit redo [-n|--dry-run] [-f|--force]`

Re-applies the operation the last `rgit undo` took back, with the same preview and the same safety check as `undo` (`--force` overrides it). Available until another state-changing command runs.

<details>
<summary>Options as printed by <code>rgit redo -h</code></summary>

```text
Bring back the operation most recently undone

Usage: rgit redo [OPTIONS]

Options:
  -n, --dry-run  Show what would be redone without changing anything
  -f, --force    Redo even if something the operation touched has changed since (overwrites it)
  -h, --help     Print help
```

</details>


### `rgit oops`

"I just did something wrong": the most likely fixes, ready to run (rgit only).

**Syntax:** `rgit oops [--all | <number>]`

An rgit-only command for the moment right after a mistake, when you don't know which command takes it back. `oops` looks at the operation log (see `rgit undo`), HEAD's reflog and the repository's state, and lists the three most likely fixes, best guess first, each with a one-line explanation and the exact commands. `rgit oops <number>` runs one: each step is a normal rgit command, printed before it runs, logged, and undoable on its own with `rgit undo`. Without anything to fix it prints `Nothing looks wrong: ...`. `--all` lists every suggestion.

What it recognizes:

- **An operation stopped halfway** (merge, rebase, cherry-pick, revert, bisect): cancel it (`merge --abort`, `rebase --abort`, ..., `bisect reset`).
- **A command just ran that you may regret** (`reset --hard`, `branch -D`, `clean`, `restore`, `commit --amend`, `rebase`, `merge`, `stash drop`, ...): `rgit undo`, with what it will put back. Riskier and more recent commands rank higher. A command whose commits are already pushed ranks lower and says the remote keeps them.
- **The last thing you did was `rgit undo`**: `rgit redo`.
- **A commit made too early**: `reset --soft HEAD~1` (changes stay staged); an amend: back to the commit before the amend. When the commit is already on the upstream: `revert --no-edit HEAD` instead of rewriting shared history.
- **Commits made directly on `main`/`master`** (or `init.defaultBranch`) that the upstream doesn't have, or without an upstream, commits made in a row in the last two hours: move them to a new branch named after the last commit's subject and put the default branch back. Uncommitted changes are stashed around the move and come along.
- **A deleted branch**: recreate it at the commit it was at (exact from the operation log; from HEAD's reflog for branches deleted with real git, when their commits are on no other ref).
- **Commits no ref reaches any more** (after a reset, an amend, a rebase, or a branch deleted outside rgit): put a `rescued-<hash>` branch on them before the reflog expires.
- **A detached HEAD**: with new commits on it, `switch -c <name>` keeps them on a branch; without, `switch` back to the branch you came from.

Every suggestion only adds refs or is undoable, so following one never loses work. `oops` never suggests `undo --force`: when something changed since the last command, that fix is left out.

<details>
<summary>Options as printed by <code>rgit oops -h</code></summary>

```text
"I just did something wrong": lists the most likely fixes; `rgit oops <n>` runs one

Usage: rgit oops [OPTIONS] [NUMBER]

Arguments:
  [NUMBER]  Run this suggestion (its number in the list) instead of listing them

Options:
      --all   List every suggestion, not just the three most likely
  -h, --help  Print help
```

</details>

**Example:**

```text
$ rgit branch -D topic
Deleted branch topic (was 8e2f1a4).
$ rgit add a.txt
$ rgit oops
Most likely fixes, best guess first:

  1) Undo your last command: `rgit add a.txt` (just now)
     Puts back what it changed: index (staging area): put back as it was.
     $ rgit undo

  2) Bring back the deleted branch 'topic' (it was at 8e2f1a4 "topic work")
     It was deleted just now and its last commits are on no other branch; this recreates it at
     the same commit.
     $ rgit branch topic 8e2f1a4c03b7

Run one with `rgit oops <number>` (it can be taken back with `rgit undo`).
$ rgit oops 2
Fix 2: Bring back the deleted branch 'topic' (it was at 8e2f1a4 "topic work")
$ rgit branch topic 8e2f1a4c03b7
Done. `rgit undo` takes it back.
```


### `rgit wip`

Save everything uncommitted on a hidden per-branch ref, leave a clean tree, bring it back later (rgit only).

**Syntax:** `rgit wip [<message>...]`, `rgit wip --list [--all]`, `rgit wip --restore [<n>]`, `rgit wip --drop [<n>]` (`rgit save` is the same command)

A one-word stash for "I'm halfway through this and need a clean tree". `rgit wip halfway through parser` (no quotes needed) saves the staged changes, the unstaged changes and the untracked files (not ignored ones) as one WIP commit, then resets the working tree and index to HEAD and deletes the saved untracked files. Unlike `stash`, untracked files are included by default and the saves belong to the branch: each branch has its own stack on `refs/rgit/wip/<branch>`, newest first (`wip@{0}`).

- `--list` (`-l`) lists the current branch's saves with their message and age; `--all` lists every branch's (also the default on a detached HEAD).
- `--restore [<n>]` (`-r`) puts save `n` (default 0) back: staged changes staged again, unstaged ones unstaged, untracked files untracked. It needs a clean working tree. If HEAD moved since the save, the changes are three-way merged onto it ("Updated upstream" / "WIP changes" conflict markers); on a conflict the save is kept, the exit code is 1, and `rgit wip --drop` removes it once you're done. It refuses before writing anything if a saved untracked file already exists.
- `--drop [<n>]` throws save `n` away.

Without changes it prints `No local changes to save`. On a detached HEAD, saving and restoring stop with `fatal: rgit wip saves per branch` (use `stash` there). Every `wip` action can be taken back with `rgit undo`.

A save is a stash-shaped commit (tree = the working tree; parents: the commit it was made on, the index commit, the untracked-files commit, then the save below it), so the whole stack stays reachable from one ref: `gc` keeps it and real git sees it too (`git log refs/rgit/wip/main`). Restoring or dropping a save in the middle rewrites the saves above it (same contents, new hashes). Saves stay under the branch name they were made on, so renaming a branch leaves them under the old name.

<details>
<summary>Options as printed by <code>rgit wip -h</code></summary>

```text
Save everything uncommitted as a WIP commit on a hidden per-branch ref and clean the tree; `--restore` brings it back

Usage: rgit wip [OPTIONS] [MESSAGE]...

Arguments:
  [MESSAGE]...  What you were in the middle of (the words are joined; no quotes needed)

Options:
  -l, --list           List this branch's WIP saves, newest first
      --all            With --list: every branch's saves
  -r, --restore [<N>]  Bring a save back (default: the newest, wip@{0}) and take it off the stack
      --drop [<N>]     Throw a save away (default: the newest)
  -h, --help           Print help
```

</details>

**Example:**

```text
$ rgit wip halfway through parser
Saved wip@{0} (WIP on main: halfway through parser)
The working tree is clean; `rgit wip --restore` brings the changes back.
$ rgit status --short
$ rgit save
No local changes to save
$ rgit wip --list
wip@{0}: WIP on main: halfway through parser (2 minutes ago)
$ rgit wip --restore
Restored wip@{0} (WIP on main: halfway through parser)
$ rgit status --short
MM a.txt
?? notes/
```


### `rgit fixup`

Fold the staged changes into an earlier commit, in one step (rgit only).

**Syntax:** `rgit fixup [<commit>] [-a] [-f]`

git needs three commands for this (`commit --fixup=<commit>`, `rebase -i --autosquash <commit>~1`, and a stash around it when the tree is dirty). `rgit fixup <commit>` takes any revision (`HEAD~2`, a hash, a tag); without one it lists the last 10 commits of the branch, numbered, and asks which (Enter cancels). Then:

- **HEAD** is simply amended (`commit --amend --no-edit`); nothing else is rewritten.
- **An older commit** gets a `fixup! <subject>` commit that is squashed into it right away: the commits from it up to HEAD are replayed with the fix folded in (new hashes, same messages and authors). Unstaged changes are stashed for the rewrite and popped afterwards.
- `-a` stages modified and deleted tracked files first, like `commit -a`. Without anything staged it stops: `nothing staged to fix up with`.
- **Pushed commits** (a remote-tracking branch already has them; the list marks them `[pushed: origin/main]`) are refused unless `-f`/`--force`, since rewriting them makes the branch diverge from the remote (you'd need `rgit push --force`).
- **Conflicts:** if the fix clashes with a later commit, the rewrite stops like a rebase does. Resolve, `rgit add`, `rgit rebase --continue` (then `rgit stash pop` if unstaged changes were stashed), or `rgit undo` to go back to before the fixup with your fix staged again.

The whole command is one `rgit undo` step. It can't fix up across a merge commit, the root commit (unless it is HEAD), or while a merge, rebase, cherry-pick, revert or `am` is in progress.

<details>
<summary>Options as printed by <code>rgit fixup -h</code></summary>

```text
Fold the staged changes into an earlier commit (picked from a list or named) and rewrite the commits on top

Usage: rgit fixup [OPTIONS] [COMMIT]

Arguments:
  [COMMIT]  The commit to fix (any revision); without it, pick from a list of recent commits

Options:
  -a, --all    Stage modified and deleted tracked files first, like `commit -a`
  -f, --force  Rewrite the commit even if a remote-tracking branch already has it
  -h, --help   Print help
```

</details>

**Example:**

```text
$ rgit add parser.rs
$ rgit fixup
Fix up which commit?
   1  9c1e2d4  docs: parser usage
   2  41ab7f0  parser: handle empty input
   3  d02f9e1  initial import  [pushed: origin/main]
Number (Enter cancels): 2
Fixed up 41ab7f0 parser: handle empty input (now 7e3a0b2), 1 later commit rewritten.
```


### `rgit absorb`

Fold each staged hunk into the commit that last changed those lines, in one rewrite (rgit only, like the `git-absorb` tool).

**Syntax:** `rgit absorb [-n] [-f]`

Where `rgit fixup` asks which commit a fix belongs to, `absorb` works it out. The staged changes are split into hunks against HEAD (no context lines); for each hunk, `blame` names the commit that last changed the lines it replaces. If that is one commit that can be rewritten (on the first-parent chain from HEAD, before any merge, the root commit, or a commit a remote-tracking branch has), the hunk goes into it. One `fixup!` commit is made per target and all are squashed in a single rewrite of the commits from the oldest target up to HEAD.

A hunk stays staged, with the reason printed, when:

- it only adds lines (nothing to blame), or it is a new, deleted or binary file, or a mode change;
- it changes lines that come from several commits;
- its lines come from a pushed commit (`-f`/`--force` rewrites those too), or from before a merge or the root commit;
- folding it in would conflict: a later commit changed the lines right next to it. This is checked with a three-way merge before anything is rewritten.

The plan is printed first; `-n`/`--dry-run` stops there. Unstaged changes and the hunks that stay staged are kept exactly as they were (they are parked in the stash during the rewrite). It is one `rgit undo` step; if the rewrite still stops on a conflict, the printed note says how to continue or go back. It refuses while a merge, rebase, cherry-pick, revert or `am` is in progress.

<details>
<summary>Options as printed by <code>rgit absorb -h</code></summary>

```text
Fold each staged hunk into the commit that last changed those lines (blame), in one rewrite

Usage: rgit absorb [OPTIONS]

Options:
  -n, --dry-run  Print which hunk goes into which commit, change nothing
  -f, --force    Also rewrite commits a remote-tracking branch already has
  -h, --help     Print help
```

</details>

**Example:**

```text
$ rgit add a.txt src/lib.txt new.txt
$ rgit absorb
Absorbing 2 of 3 staged hunks into 2 commits (rewriting the last 3 commits):
  a.txt:2        -> 41ab7f0 second commit
  src/lib.txt:1  -> d02f9e1 third commit
Staying staged:
  new.txt        new file (nothing to blame)
Successfully rebased and updated refs/heads/main.
Absorbed 2 hunks into 2 commits; 1 hunk still staged.
```


### `rgit split`

Split one commit into several by hunk, picking each new commit's hunks and message (rgit only).

**Syntax:** `rgit split [<commit>] [-f]`

`rgit banana-split` splits a commit by path or directory; `split` goes down to hunks, so two changes in the same file can end up in different commits. git's way is `git rebase -i` with `edit` on the commit, `git reset HEAD~`, then `git add -p` and `git commit` once per new commit, then `git rebase --continue`.

The commit (default: HEAD) is cut into pieces: one per hunk of a changed text file (no context lines), and one whole-file piece for each new, deleted or binary file and each mode change. Then, commit by commit, each remaining piece is shown and you answer `y` (take it into this commit), `n` (leave it for a later one), `a` / `d` (take / leave it and all the rest) or `q` (quit, nothing changes). Each new commit asks for its message: Enter gives `<subject> (part N)`, or for the last commit the original message. The last piece left goes into the last commit without asking. The first commit must take at least one piece and leave at least one.

The new commits keep the original author. The last one ends at the original tree, so the commits on top keep their trees too and are only re-parented: no replay, no conflicts, and the working tree and index (including uncommitted changes) are never touched. It is one `rgit undo` step. It refuses a merge commit, the root commit, a commit with a merge between it and HEAD, a commit a remote-tracking branch has (unless `-f`/`--force`), and runs while a merge, rebase, cherry-pick, revert or `am` is in progress.

<details>
<summary>Options as printed by <code>rgit split -h</code></summary>

```text
Split one commit into several by hunk, picking each new commit's hunks and message

Usage: rgit split [OPTIONS] [COMMIT]

Arguments:
  [COMMIT]  The commit to split (default: HEAD)

Options:
  -f, --force  Also split a commit a remote-tracking branch already has
  -h, --help   Print help
```

</details>

**Example:**

```text
$ rgit split HEAD~1
Splitting 8c41d2e mixed bag (4 hunks).
Pick the hunks of each new commit; the ones you leave go on to the next commit.

Commit 1: y = take the hunk, n = leave it, a = take it and the rest, d = leave it and the rest, q = quit (nothing changes)

a.txt:1
-one
+ONE
 two
Into commit 1 [y,n,a,d,q]? y

a.txt:+2
 one
 two
+three
Into commit 1 [y,n,a,d,q]? n
...
Message for commit 1 (Enter: "mixed bag (part 1)"): Uppercase one, add c
...
The last hunk goes into commit 3.
Message for commit 3 (Enter: "mixed bag"):

Split 8c41d2e into 3 commits:
  5e0b7a3 Uppercase one, add c
  a91f4c2 mixed bag (part 2)
  0d6e8b1 mixed bag
Rewrote the commit on top; the files are unchanged.
```


### `rgit whereami`

One screen on where you are: branch, upstream, last commit, changes, operation in progress, saves, and what to run next (rgit only).

**Syntax:** `rgit whereami`

It shows what `git status`, `git branch -vv`, `git log -1` and `git stash list` would show between them, in one place, plus one suggestion. The lines:

- **Header:** the branch (`(no commits yet)` when unborn), or `HEAD detached at <commit>`, with `(rebasing <branch>)` / `(bisecting, started on <branch>)` when a rebase or bisect detached it.
- **Upstream:** `origin/main: 2 ahead, 1 behind` (counted against the remote-tracking ref, so as of the last fetch; nothing is fetched), `up to date`, `none (not pushed to origin yet)`, `none (no remotes)`, or `gone` when the remote branch was deleted.
- **Last commit:** hash, subject, author and age.
- **Changes:** conflicted, staged, modified and untracked counts, or `none (clean)`.
- **In progress:** a merge, rebase (with how many commits are done), cherry-pick, revert, bisect (the commit under test), `am`, or a `stash apply` stopped on conflicts. Only shown when there is one.
- **Saved:** stash entries and this branch's `rgit wip` saves. Only shown when there are any.
- **Next:** the most likely next command, in this order: finish (or abort) the operation in progress (resolving conflicts first); get off a detached HEAD; make the first commit; commit what is staged, or stage what is modified; `rgit sync` when behind, `rgit push` when ahead, `rgit push -u` for a branch never pushed; add untracked files; otherwise nothing to do.

It only reads, so it is not recorded in the operation log.

<details>
<summary>Options as printed by <code>rgit whereami -h</code></summary>

```text
One screen on where you are: branch, upstream, last commit, changes, operation in progress, saves, and what to run next

Usage: rgit whereami

Options:
  -h, --help  Print help
```

</details>

**Example:**

```text
$ rgit whereami
On branch main
Upstream    origin/main: 1 ahead, 1 behind
Last commit 4332f6c clone commit (Test Author, 2 hours ago)
Changes     none (clean)
Saved       1 stash entry
Next        `rgit sync` to bring in origin/main's new commit and replay yours on top

$ rgit whereami
HEAD detached at 8f0c2e1 (rebasing main)
Last commit 8f0c2e1 side edit (Test Author, 5 minutes ago)
Changes     1 conflicted
In progress rebase of main onto 8f0c2e1: 0 of 1 commits done
Next        resolve the conflicts (`rgit conflicts` shows them), `rgit add` each file, then `rgit rebase --continue`
```


### `rgit sync`

Bring the current branch level with its upstream (and push), in one command with a plan (rgit only).

**Syntax:** `rgit sync [-p] [--rebase | --merge] [-n] [--no-fetch]`

git's version is `git fetch`, then `git rebase @{upstream}` (or `git pull --rebase`), stashing around it when the tree is dirty, then `git push`. `rgit sync` fetches the branch's upstream (`branch.<name>.remote` / `.merge`), prints where the branch stands and what it will do, then does it:

- **Up to date / only ahead:** nothing to bring in; with `-p`/`--push` the local commits are pushed, otherwise it says how many are not pushed yet.
- **Only behind:** a fast-forward.
- **Diverged:** the local commits are rebased onto the upstream (the default), or with `--merge` the upstream is merged in. Without either flag `branch.<name>.rebase`, then `pull.rebase` decide (`false` = merge). Either way the branch then contains the upstream, so `--push` is a plain fast-forward push, never a force push.
- **No upstream yet:** `--push` publishes the branch to `branch.<name>.remote` (else `origin`) and tracks it; without `--push` it only says so. The same goes for an upstream deleted on the remote.

Staged and unstaged changes are stashed around the fast-forward, rebase or merge and popped afterwards (untracked files stay put). `-n`/`--dry-run` fetches and prints the plan without changing anything else; `--no-fetch` plans against the remote-tracking branch as it is. On a conflict it stops like `rebase`/`merge` do and prints how to go on, or `rgit undo` goes back to before the sync (a push already made stays on the remote). It refuses on a detached HEAD and while a merge, rebase, cherry-pick, revert or `am` is in progress.

<details>
<summary>Options as printed by <code>rgit sync -h</code></summary>

```text
Fetch the branch's upstream, print the plan, then fast-forward, rebase or merge onto it (and push with --push)

Usage: rgit sync [OPTIONS]

Options:
  -p, --push      Also push the result to the upstream (publishing the branch if it has none yet)
      --rebase    Rebase local commits onto the upstream (the default, unless pull.rebase=false)
      --merge     Merge the upstream into the branch instead of rebasing
  -n, --dry-run   Fetch and print the plan, change nothing else
      --no-fetch  Skip the fetch: plan against the remote-tracking branch as it is
  -h, --help      Print help
```

</details>

**Example:**

```text
$ rgit sync --push
From https://github.com/you/project
   4f1f653..9228448  main             -> origin/main
'main' and origin/main have diverged: 1 commit here, 1 commit there.
Plan:
  - rebase the local commit onto origin/main
  - push to origin/main
Successfully rebased and updated refs/heads/main.
To https://github.com/you/project
   9228448..8005fed  main -> main
'main' is in sync with origin/main.
```


### `rgit reset`

Move HEAD and optionally reset the index and working tree.

**Arguments and flags:** `--soft`, `--mixed`, `--hard`, `[-- <paths...>]`

Moves HEAD, restores the index, and synchronizes the working tree. A mixed or path reset prints git's `Unstaged changes after reset:` list (`M`/`D`, and `U` for paths still unmerged); `--mixed`/`--hard` clear conflicts and end a merge, cherry-pick or revert; `--soft` refuses in the middle of a merge.

**Patch mode (`reset -p [<tree-ish>]`).** Same prompts as `add -p`, but unstages the hunks you accept.

<details>
<summary>Options as printed by <code>rgit reset -h</code></summary>

```text
Usage: rgit reset [OPTIONS] [COMMIT] [-- <PATHS>...]

Arguments:
  [COMMIT]    
  [PATHS]...  

Options:
      --soft   
      --mixed  
      --hard   
  -p, --patch  
  -h, --help   Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit reset --soft HEAD~1
rgit reset --hard HEAD
```


### `rgit stash`

Set work aside and bring it back later.

**Syntax:** `rgit stash [push [-m <msg>] [-p] [-u] [-a] [-- <path>...]] | pop [<stash>] | apply [<stash>] | list | drop [<stash>] | show [-p] [<stash>] | clear`

Sets your uncommitted changes aside and restores a clean working tree. `push` is the default when you run `rgit stash` alone; `-u` also stashes untracked files, `-a` ignored ones too, `-p` lets you pick hunks. `pop` re-applies the newest stash and drops it, `apply` keeps it, `list` shows all, `show` prints a stash as a patch (`-p` is accepted and has the same effect), `drop`/`clear` delete. Saving refuses on an unmerged index.

**Patch mode (`stash [push] -p`).** Same prompts as `add -p`; only the chosen hunks are stashed and removed from the working tree. The index is kept (git's implied `--keep-index`).

<details>
<summary>Options as printed by <code>rgit stash -h</code></summary>

```text
Enum of subcommands supported by the stash command

Usage: rgit stash [OPTIONS] [COMMAND]

Commands:
  push   
  pop    
  apply  
  list   
  drop   
  show   
  clear  

Options:
  -p, --patch              
  -u, --include-untracked  
  -a, --all                
  -h, --help               Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo wip >> a.txt
rgit stash
rgit stash list
rgit stash show -p
rgit stash pop
```


### `rgit reflog`

Show where HEAD and branches used to point.

**Arguments and flags:** `show [<ref>] [-n <count>]`, `exists <ref>`, `delete <ref>@{<n>}...`, `expire [--all] [--expire <days|now|never>] [<ref>]`

Records every move of `HEAD`, branches, and remote-tracking branches so lost commits can be recovered (e.g. `reset --hard HEAD@{1}`).

<details>
<summary>Options as printed by <code>rgit reflog -h</code></summary>

```text
Usage: rgit reflog [OPTIONS] [COMMAND]

Commands:
  show    
  exists  
  delete  
  expire  

Options:
  -n, --max-count <MAX_COUNT>  
  -h, --help                   Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit reflog
rgit reflog -n 2
rgit reflog show main
```


### `rgit bisect`

Binary-search the history for the commit that broke something.

**Arguments and flags:** `start`, `bad`, `good`, `skip`, `reset`, `log`, `run`

Automated binary search to locate the regression-introducing commit.

<details>
<summary>Options as printed by <code>rgit bisect -h</code></summary>

```text
Enum of subcommands supported by the bisect command

Usage: rgit bisect <COMMAND>

Commands:
  start  
  bad    
  good   
  skip   
  reset  
  log    
  run    

Options:
  -h, --help  Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit bisect start
rgit bisect bad
rgit bisect good HEAD~1
rgit bisect reset
```


### `rgit fsck`

Check the object database for corruption.

**Arguments and flags:** `--connectivity-only`, `--no-dangling`, `--unreachable`, `--no-reflogs`

Verifies object integrity and connectivity, validates refs and the index, and reports dangling or unreachable objects.

<details>
<summary>Options as printed by <code>rgit fsck -h</code></summary>

```text
Usage: rgit fsck [OPTIONS]

Options:
      --connectivity-only  
      --no-dangling        
      --unreachable        
      --no-reflogs         
  -h, --help               Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit fsck
```


### `rgit gc`

Clean up: expire old reflog entries and delete unreachable objects.

**Arguments and flags:** `--prune <days|now|never>`, `--reflog-expire <days|now|never>`, `-n|--dry-run`

Expires old reflog entries and deletes unreachable loose objects past the prune window, keeping anything reachable from refs, the index, the stash, in-progress operations, or live reflogs.

<details>
<summary>Options as printed by <code>rgit gc -h</code></summary>

```text
Usage: rgit gc [OPTIONS]

Options:
      --prune <PRUNE>                  [default: 14]
      --reflog-expire <REFLOG_EXPIRE>  [default: 90]
  -n, --dry-run                        
  -h, --help                           Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit gc --dry-run
rgit gc
```


### `rgit prune`

Delete unreachable loose objects.

**Arguments and flags:** `-n`, `-v`, `--expire <time>`, `[--] <head>...`

Deletes every unreachable loose object (git's default; `--expire` keeps newer ones and whatever they refer to), using the same roots as `gc` (refs, HEAD, index, stash, operations in progress, all reflog entries, every worktree) plus any `<head>` given. Also removes stale `tmp_*` files and reports stray files in `.git/objects`. Silent by default; `-n`/`-v` print `<hash> <type>` per object exactly like git. Refuses to touch a damaged repository or one with packfiles.

<details>
<summary>Options as printed by <code>rgit prune -h</code></summary>

```text
Usage: rgit prune [OPTIONS] [HEAD]...

Arguments:
  [HEAD]...  

Options:
  -n, --dry-run        
  -v, --verbose        
      --progress       Accepted for compatibility; prune shows no progress
      --expire <TIME>  
  -h, --help           Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit prune -n
```


## 7. Remotes and collaboration

Share work through a local path or through GitHub.


### `rgit remote`

Manage the remotes this repository knows.

**Arguments and flags:** `[-v|--verbose]`, `add`, `remove`/`rm`, `rename`, `get-url`, `set-url`, `show`

Manages tracked remotes, URLs, fetch refspecs, and remote-tracking references.

<details>
<summary>Options as printed by <code>rgit remote -h</code></summary>

```text
Subcommands supported by the `remote` command for managing tracking repositories

Usage: rgit remote [OPTIONS] [COMMAND]

Commands:
  add      
  remove   
  rm       
  rename   
  get-url  
  set-url  
  show     

Options:
  -v, --verbose  
  -h, --help     Print help
```

</details>

**Examples** (run in a clone of `repo` (made with `rgit clone repo mine`))

```bash
rgit remote -v
rgit remote add backup ../repo
rgit remote show origin
rgit remote rename backup old
rgit remote remove old
```


### `rgit fetch`

Download new commits from a remote.

**Syntax:** `rgit fetch [<remote>] [<branch>]`

Walks remote commit graphs, downloads missing objects, updates remote-tracking refs (`refs/remotes/<remote>/<branch>`), and writes `FETCH_HEAD`.

<details>
<summary>Options as printed by <code>rgit fetch -h</code></summary>

```text
Usage: rgit fetch [REMOTE] [BRANCH]

Arguments:
  [REMOTE]  
  [BRANCH]  

Options:
  -h, --help  Print help
```

</details>

**Examples** (run in a clone of `repo` (made with `rgit clone repo mine`))

```bash
rgit fetch
rgit fetch origin main
```


### `rgit push`

Upload commits to a remote.

**Arguments and flags:** [<remote>] [<branch>] `-f|--force`, `-u|--set-upstream`

Computes missing objects, transfers blobs/trees/commits, enforces fast-forward checks, and updates remote references.

<details>
<summary>Options as printed by <code>rgit push -h</code></summary>

```text
Usage: rgit push [OPTIONS] [REMOTE] [REFSPEC]

Arguments:
  [REMOTE]   
  [REFSPEC]  

Options:
  -f, --force         
  -u, --set-upstream  
  -h, --help          Print help
```

</details>

**Examples** (run in a clone of `repo` (made with `rgit clone repo mine`))

```bash
rgit switch -c topic
echo x >> a.txt
rgit commit -a -m "work"
rgit push -u origin topic
```


### `rgit pull`

Fetch from a remote and merge it into the current branch.

**Syntax:** `rgit pull [<remote>] [<branch>]`

Seamless convenience command composing `fetch` with upstream tracking resolution and `merge`.

<details>
<summary>Options as printed by <code>rgit pull -h</code></summary>

```text
Usage: rgit pull [REMOTE] [BRANCH]

Arguments:
  [REMOTE]  
  [BRANCH]  

Options:
  -h, --help  Print help
```

</details>

**Examples** (run in a clone of `repo` (made with `rgit clone repo mine`))

```bash
rgit pull
```


### `rgit bundle`

Pack history into a single file for offline transfer.

**Arguments and flags:** `create [-q] [--version=2|3] <file> <rev-list-args>`, `verify [-q] <file>`, `list-heads <file> [<refname>...]`, `unbundle <file> [<refname>...]`

Moves history offline in git's bundle format, so bundles work with real git in both directions (`git clone`/`git fetch` from an rgit bundle, `rgit bundle unbundle` of a git bundle). `create` accepts refs, `^rev`, `A..B`, `A...B`, `--not`, `--all|--branches|--tags|--remotes`, records the prerequisite commits a receiver must already have, and packs every needed object (undeltified, so bundles are larger than git's); `verify` checks prerequisites and describes the bundle; `unbundle` stores the objects as loose objects and prints the refs without updating any.

<details>
<summary>Options as printed by <code>rgit bundle -h</code></summary>

```text
Subcommands of `bundle`, which packs commits and the objects they need into a single file for offline transfer, and inspects or unpacks such files

Usage: rgit bundle <COMMAND>

Commands:
  create      Write a bundle with the commits selected by <rev-list-args> (e.g. `main`, `--all`, `v1..main`)
  verify      Check that a bundle is valid and can be applied to this repository
  list-heads  List the refs a bundle contains (only the named ones, if any are given)
  unbundle    Store a bundle's objects in this repository and print its refs

Options:
  -h, --help  Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit bundle create ../repo.bundle --all
rgit bundle verify ../repo.bundle
rgit bundle list-heads ../repo.bundle
```


### `rgit request-pull`

Write a pull-request summary.

**Syntax:** `rgit request-pull [-p] <start> <url> [<end>]`

Writes a pull request summary: base commit, repository and ref to pull, tip commit, tag message or branch description, a shortlog of `<start>..<end>` grouped by author (through `.mailmap`), and a diffstat with creations, deletions and mode changes (`-p` adds the patch). Warns (exit 1) when `<url>` has no ref named like `<end>` at that commit. Renames show as delete + create.

<details>
<summary>Options as printed by <code>rgit request-pull -h</code></summary>

```text
usage: rgit request-pull [options] start url [end]

    -p                    show patch text as well
```

</details>


### `rgit fmt-merge-msg`

Build a merge commit message from FETCH_HEAD-style lines.

**Arguments and flags:** `-m <message>`, `--into-name <branch>`, `--log[=<n>]`, `--no-log`, `-F <file>`

Builds the default merge commit message from `FETCH_HEAD`-style lines on standard input: `Merge branch 'x' of <repo> into <branch>` (no ` into` for `main`/`master` or `merge.suppressDest` patterns), merged tag messages, and with `--log` / `merge.log` a per-tip list of new commit subjects with `# By` / `# Via` credit lines.

<details>
<summary>Options as printed by <code>rgit fmt-merge-msg -h</code></summary>

```text
usage: rgit fmt-merge-msg [-m <message>] [--log[=<n>] | --no-log] [--file <file>]

    --[no-]log[=<n>]      populate log with at most <n> entries from shortlog
    -m, --[no-]message <text>
                          use <text> as start of message
    --[no-]into-name <name>
                          use <name> instead of the real target branch
    -F, --[no-]file <file>
                          file to read from
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
printf '%s\t\tbranch '"'"'feature'"'"' of .\n' $(rgit rev-parse feature) | rgit fmt-merge-msg
```


## 8. Patches and email

Move changes as text: patch files, mailboxes and trailers.


### `rgit apply`

Apply a patch file to the working tree and/or the index.

**Arguments and flags:** `--check`, `--stat`, `--numstat`, `--summary`, `--apply`, `--cached`, `--index`, `-R`, `-v`, `-p<n>`, `[<patch>...]`

Applies unified or git-style diffs (including `format-patch` mail) to the working tree and/or index, all-or-nothing, with offset hunk placement, creations, deletions, renames, copies, mode changes, `GIT binary patch` literals and deltas, and CRLF/no-final-newline preservation.

<details>
<summary>Options as printed by <code>rgit apply -h</code></summary>

```text
Usage: rgit apply [OPTIONS] [PATCH]...

Arguments:
  [PATCH]...  

Options:
      --check    
      --stat     
      --numstat  
      --summary  
      --apply    
      --cached   
      --index    
  -R, --reverse  
  -v, --verbose  
  -p <N>         [default: 1]
  -h, --help     Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo more >> a.txt
rgit diff > change.patch
rgit restore a.txt
rgit apply --check change.patch
rgit apply change.patch
```


### `rgit format-patch`

Export commits as email-style patch files.

**Arguments and flags:** `-<n>`, `--root`, `--stdout`, `-o <dir>`, `-n|-N`, `--start-number`, `--subject-prefix`, `-k`, `--signature`/`--no-signature`, `<since>|<A..B>`

Exports commits as mailbox patches (`NNNN-subject.patch`) with RFC 2047 headers, diffstat and summary, git-identical hunks (function-name headers, indent heuristic), and binary literals.

<details>
<summary>Options as printed by <code>rgit format-patch -h</code></summary>

```text
Usage: rgit format-patch [OPTIONS] [REVISION]...

Arguments:
  [REVISION]...  

Options:
      --max-count <MAX_COUNT>                
      --root                                 
      --stdout                               
  -o, --output-directory <OUTPUT_DIRECTORY>  
  -n, --numbered                             
  -N, --no-numbered                          
      --start-number <START_NUMBER>          [default: 1]
      --subject-prefix <SUBJECT_PREFIX>      [default: PATCH]
  -k, --keep-subject                         
      --signature <SIGNATURE>                
      --no-signature                         
  -h, --help                                 Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit format-patch -1
rgit format-patch main..feature -o patches
```


### `rgit am`

Apply a mailbox of patches as commits.

**Arguments and flags:** `[<mbox>...]`, `-k`, `--continue`, `--skip`, `--abort`, `--quit`, `--show-current-patch[=raw|diff]`

Applies a mailbox of patches as commits that keep the original author, date, and message; stops on conflicts with resumable state in `.git/rebase-apply/`.

<details>
<summary>Options as printed by <code>rgit am -h</code></summary>

```text
Usage: rgit am [OPTIONS] [MBOX]...

Arguments:
  [MBOX]...  

Options:
  -r, --continue                         
      --skip                             
      --abort                            
      --quit                             
      --show-current-patch[=<raw|diff>]  
  -k, --keep-subject                     
  -h, --help                             Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit format-patch -1 --stdout > p.mbox
rgit reset --hard HEAD~1
rgit am p.mbox
```


### `rgit mailsplit`

Split a mailbox into one file per message.

**Arguments and flags:** `-o<directory>`, `-d<prec>`, `-f<n>`, `-b`, `--keep-cr`, `--mboxrd`, `[<mbox>|<Maildir>...]`

Splits mbox files, Maildirs or standard input into one numbered file per message (`0001`, `0002`, ...) and prints the count.

<details>
<summary>Options as printed by <code>rgit mailsplit -h</code></summary>

```text
usage: rgit mailsplit [-d<prec>] [-f<n>] [-b] [--keep-cr] [--mboxrd] -o<directory> [(<mbox>|<Maildir>)...]
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit format-patch -2 --stdout > p.mbox
mkdir split
rgit mailsplit -osplit p.mbox
```


### `rgit mailinfo`

Extract the commit message and patch from one email.

**Arguments and flags:** `-k`, `-b`, `-m|--message-id`, `-u`, `-n`, `--encoding=<encoding>`, `--[no-]scissors`, `--no-inbody-headers`, `--quoted-cr=<action>`, `<msg> <patch>`

Reads one email on standard input, writes its commit message and patch to the two files, and prints `Author:` / `Email:` / `Subject:` / `Date:`. Decodes RFC 2047 headers, MIME multipart, base64, quoted-printable and `format=flowed`. Charset conversion covers UTF-8, US-ASCII, ISO-8859-1/15 and Windows-1252; `--encoding` re-codes to the named charset as documented (git 2.43 itself turns re-coding off there).

<details>
<summary>Options as printed by <code>rgit mailinfo -h</code></summary>

```text
usage: rgit mailinfo [<options>] <msg> <patch> < mail >info

    -k                    keep subject
    -b                    keep non patch brackets in subject
    -m, --message-id      copy Message-ID to the end of commit message
    -u                    re-code metadata to i18n.commitEncoding
    -n                    disable charset re-coding of metadata
    --encoding <encoding> re-code metadata to this encoding
    --scissors            use scissors
    --quoted-cr <action>  action when quoted CR is found
    --no-inbody-headers   use headers in message's body
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit format-patch -1 --stdout > p.mbox
rgit mailinfo msg.txt patch.txt < p.mbox
```


### `rgit interpret-trailers`

Add or parse trailers such as Signed-off-by.

**Arguments and flags:** `--in-place`, `--trim-empty`, `--trailer <token>[(=|:)<value>]`, `--where <placement>`, `--if-exists <action>`, `--if-missing <action>`, `--only-trailers`, `--only-input`, `--unfold`, `--parse`, `--no-divider`, `[<file>...]`

Adds or parses the `Token: value` trailers at the end of commit messages, honouring `trailer.separators`, `trailer.where`, `trailer.ifexists`, `trailer.ifmissing` and `trailer.<token>.key|command|cmd|where|ifexists|ifmissing`.

<details>
<summary>Options as printed by <code>rgit interpret-trailers -h</code></summary>

```text
usage: rgit interpret-trailers [--in-place] [--trim-empty]
                               [(--trailer (<key>|<keyAlias>)[(=|:)<value>])...]
                               [--parse] [<file>...]

    --[no-]in-place       edit files in place
    --[no-]trim-empty     trim empty trailers
    --[no-]where <placement>
                          where to place the new trailer
    --[no-]if-exists <action>
                          action if trailer already exists
    --[no-]if-missing <action>
                          action if trailer is missing
    --[no-]only-trailers  output only the trailers
    --[no-]only-input     do not apply trailer.* configuration variables
    --[no-]unfold         reformat multiline trailer values as single-line values
    --parse               alias for --only-trailers --only-input --unfold
    --[no-]divider        do not treat "---" as the end of input
    --[no-]trailer <trailer>
                          trailer(s) to add
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo 'Fix the bug' | rgit interpret-trailers --trailer 'Signed-off-by: Ada <ada@example.com>'
```


### `rgit check-mailmap`

Show the canonical name and email from .mailmap.

**Arguments and flags:** `--stdin`, `--mailmap-file <file>`, `--mailmap-blob <blob>`, `<contact>...`

Prints the canonical `Name <email>` of each contact according to `.mailmap`, `mailmap.blob` and `mailmap.file`.

<details>
<summary>Options as printed by <code>rgit check-mailmap -h</code></summary>

```text
usage: rgit check-mailmap [<options>] <contact>...

    --[no-]stdin          also read contacts from stdin
    --[no-]mailmap-file <file>
                          read additional mailmap entries from file
    --[no-]mailmap-blob <blob>
                          read additional mailmap entries from blob
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo 'Ada <ada@example.com> <old@example.com>' > .mailmap
rgit check-mailmap '<old@example.com>'
```


### `rgit patch-id`

Compute stable IDs for patches.

**Arguments and flags:** `--stable`, `--unstable`, `--verbatim`

Reads patches on standard input and prints `<patch-id> <commit-id>` for each: a SHA-1 of the diff without line numbers and whitespace, so the same change always gets the same ID. Defaults come from `patchid.stable` / `patchid.verbatim`.

<details>
<summary>Options as printed by <code>rgit patch-id -h</code></summary>

```text
usage: rgit patch-id [--stable | --unstable | --verbatim]

    --unstable            use the unstable patch-id algorithm
    --stable              use the stable patch-id algorithm
    --verbatim            don't strip whitespace from the patch
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit show HEAD | rgit patch-id
```


## 9. Export and import

Archives and streams for moving history between tools.


### `rgit archive`

Export a tree as a tar, tar.gz or zip file.

**Arguments and flags:** `[-f|--format <fmt>]`, `[-l|--list]`, `[--prefix <prefix>]`, `[-o|--output <file>]`, `<tree-ish> [<paths...>]`

Exports files from a tree, commit, tag, or ref into a `tar`, `tar.gz`/`tgz`, or `zip` archive with directory hierarchy and commit timestamps preserved.

<details>
<summary>Options as printed by <code>rgit archive -h</code></summary>

```text
Usage: rgit archive [OPTIONS] [TREE-ISH] [PATH]...

Arguments:
  [TREE-ISH]  
  [PATH]...   

Options:
  -f, --format <FORMAT>  
  -l, --list             
      --prefix <PREFIX>  
  -o, --output <OUTPUT>  
  -h, --help             Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit archive -o snapshot.zip HEAD
rgit archive --format=tar.gz --prefix=proj/ -o proj.tar.gz HEAD
```


### `rgit get-tar-commit-id`

Read the commit ID stored in an archive.

**Syntax:** `rgit get-tar-commit-id < archive.tar`

Reads a tar archive from standard input and prints the commit ID recorded in it. `archive` writes it in git's pax global header whenever you archive a commit.

<details>
<summary>Options as printed by <code>rgit get-tar-commit-id -h</code></summary>

```text
usage: rgit get-tar-commit-id
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit archive -o s.tar HEAD
rgit get-tar-commit-id < s.tar
```


### `rgit fast-export`

Write history as a fast-import stream.

**Arguments and flags:** `--all | <rev>...`, `--signed-tags=<mode>`, `--no-data`, `--full-tree`, `--export-marks`, `--import-marks`, `--use-done-feature`, `--mark-tags`, `--reference-excluded-parents`, `--show-original-ids`

Writes history as a `fast-import` stream, matching git's output.

<details>
<summary>Options as printed by <code>rgit fast-export -h</code></summary>

```text
usage: rgit fast-export [<rev-list-opts>]

    --signed-tags <when>  select handling of signed tags
    --tag-of-filtered-object <when>
                          select handling of tags that tag filtered objects
    --export-marks <file> dump marks to this file
    --import-marks <file> import marks from this file
    --import-marks-if-exists <file>
                          import marks from this file if it exists
    --use-done-feature    use the done feature to terminate the stream
    --no-data             skip output of blob data
    --full-tree           output full tree for each commit
    --mark-tags           label tags with mark ids
    --reference-excluded-parents
                          reference parents which are not in fast-export stream by object id
    --show-original-ids   show original object ids of blobs/commits
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit fast-export --all > dump.fi
```


### `rgit fast-import`

Read a fast-import stream into the repository.

**Arguments and flags:** `--force`, `--stats`, `--done`, `--date-format`, `--export-marks`, `--import-marks`

Replays such a stream (`blob`, `commit` with `M`/`D`/`C`/`R`/`N`/`deleteall`, `tag`, `reset`, `alias`, ...), producing the same object ids as git.

<details>
<summary>Options as printed by <code>rgit fast-import -h</code></summary>

```text
fatal: not a git repository (or any of the parent directories): .git
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit fast-export --all > dump.fi
mkdir ../copy
(cd ../copy && rgit init && rgit fast-import < ../repo/dump.fi)
```


## 10. Objects, refs and the index (plumbing)

Low-level building blocks, mostly for scripts and for understanding how git works.


### `rgit hash-object`

Compute (and optionally store) an object's hash.

**Arguments and flags:** `-w <file>`

Computes object SHA-1 hash and optionally writes compressed object to store.

<details>
<summary>Options as printed by <code>rgit hash-object -h</code></summary>

```text
Usage: rgit hash-object [OPTIONS] <FILE>

Arguments:
  <FILE>  

Options:
  -w          
  -h, --help  Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit hash-object a.txt
rgit hash-object -w a.txt
```


### `rgit cat-file`

Show an object's contents, type or size.

**Syntax:** `rgit cat-file (-p | -t | -s | -e) <object>`

Shows an object. `-p` prints its contents (a tree as `ls-tree` lines), `-t` its type, `-s` its size in bytes. `-e` prints nothing and only sets the exit status: 0 if the object exists, 1 if the name is well formed but missing, 128 if it is not a valid name. `<object>` can be any revision: `HEAD`, `main~2`, `HEAD:path/to/file`, or an abbreviated hash.

<details>
<summary>Options as printed by <code>rgit cat-file -h</code></summary>

```text
Usage: rgit cat-file [OPTIONS] <OBJECT_HASH>

Arguments:
  <OBJECT_HASH>  

Options:
  -p          
  -t          
  -s          
  -e          
  -h, --help  Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit cat-file -t HEAD
rgit cat-file -s HEAD
rgit cat-file -p HEAD
rgit cat-file -p HEAD:a.txt
rgit cat-file -e HEAD
```


### `rgit write-tree`

Write the index as a tree object.

Serializes current staging index into tree objects; refuses on an unmerged index with git's `<path>: unmerged (<oid>)` list.

<details>
<summary>Options as printed by <code>rgit write-tree -h</code></summary>

```text
Usage: rgit write-tree

Options:
  -h, --help  Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit write-tree
```


### `rgit ls-tree`

List the contents of a tree.

**Syntax:** `rgit ls-tree [-r] [--name-only] <tree-ish> [<path>...]`

Lists a tree's entries as `<mode> <type> <hash><TAB><name>`. `<tree-ish>` is any revision that leads to a tree (a commit, a tag, `HEAD:dir`). `-r` descends into subfolders and lists only files, with full paths. `<path>` arguments filter like git: `dir` shows the entry `dir` itself, `dir/` shows what is inside it.

<details>
<summary>Options as printed by <code>rgit ls-tree -h</code></summary>

```text
Usage: rgit ls-tree [OPTIONS] <TREE_HASH> [PATHS]...

Arguments:
  <TREE_HASH>  
  [PATHS]...   

Options:
      --name-only  
  -r               
  -h, --help       Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit ls-tree HEAD
rgit ls-tree -r --name-only HEAD
rgit ls-tree HEAD d/
```


### `rgit commit-tree`

Create a commit object from a tree.

**Syntax:** `rgit commit-tree <tree> [-p <parent>] -m <msg>`

Directly writes a commit object from tree and parent hashes.

<details>
<summary>Options as printed by <code>rgit commit-tree -h</code></summary>

```text
Usage: rgit commit-tree [OPTIONS] -m <MESSAGE> <TREE_HASH>

Arguments:
  <TREE_HASH>  

Options:
  -p <PARENT_HASH>  
  -m <MESSAGE>      
  -h, --help        Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit commit-tree HEAD^{tree} -p HEAD -m "manual commit"
```


### `rgit mktree`

Build a tree from `ls-tree` formatted text.

**Arguments and flags:** `-z`, `--missing`, `--batch`

Builds tree objects from `ls-tree` formatted input.

<details>
<summary>Options as printed by <code>rgit mktree -h</code></summary>

```text
usage: rgit mktree [-z] [--missing] [--batch]

    -z                    input is NUL terminated
    --[no-]missing        allow missing objects
    --[no-]batch          allow creation of more than one tree
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit ls-tree HEAD | rgit mktree
```


### `rgit mktag`

Validate and store a tag object.

**Arguments and flags:** `--no-strict`

Validates tag object text from standard input with git's fsck checks and stores it.

<details>
<summary>Options as printed by <code>rgit mktag -h</code></summary>

```text
usage: rgit mktag

    --[no-]strict         enable more strict checking
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
printf 'object %s\ntype commit\ntag v1\ntagger A <a@b.c> 1700000000 +0000\n\nmessage\n' $(rgit rev-parse HEAD) | rgit mktag
```


### `rgit unpack-file`

Write a blob to a temporary file.

**Syntax:** `rgit unpack-file <blob>`

Writes a blob to a `.merge_file_XXXXXX` temporary file and prints its name.

<details>
<summary>Options as printed by <code>rgit unpack-file -h</code></summary>

```text
usage: rgit unpack-file <blob>
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit unpack-file $(rgit rev-parse HEAD:a.txt)
```


### `rgit rev-parse`

Resolve revisions and query repository properties.

**Arguments and flags:** `--git-dir`, `--show-toplevel`, `--is-inside-work-tree`, `--is-bare-repository`, `--abbrev-ref`, `--symbolic-full-name`, `--verify [-q]`, `--short[=N]`, `--sq`, `<rev>...`

Resolves revision syntax (`HEAD~2`, `HEAD^`, `@`, mixed chains like `topic~2^0`, branches, tags, `heads/<name>` / `tags/<name>`, short hashes, `<ref>@{n}`, `^{type}`; a tag wins over a same-named branch, as in git) to object hashes or symbolic names, and queries repository properties.

<details>
<summary>Options as printed by <code>rgit rev-parse -h</code></summary>

```text
Usage: rgit rev-parse [OPTIONS] [REVS]...

Arguments:
  [REVS]...  

Options:
      --git-dir              
      --show-toplevel        
      --is-inside-work-tree  
      --is-bare-repository   
      --abbrev-ref           
      --symbolic-full-name   
      --verify               
  -q, --quiet                
      --short[=<N>]          
      --sq                   
  -h, --help                 Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit rev-parse HEAD
rgit rev-parse --short HEAD
rgit rev-parse --abbrev-ref HEAD
rgit rev-parse --show-toplevel
rgit rev-parse HEAD~1^{tree}
```


### `rgit rev-list`

List commits reachable from given revisions.

**Arguments and flags:** `<rev>...`, `^rev`, `A..B`, `A...B`, `--not`, `--all|--branches|--tags|--remotes`, `-n|--max-count`, `--skip`, `--reverse`, `--count`, `--topo-order|--date-order`, `--first-parent`, `--merges|--no-merges|--min-parents|--max-parents`, `--since|--until`, `--author|--committer|--grep` (`-i`, `-F`, `--all-match`, `--invert-grep`), `--parents`, `--timestamp`, `--left-right`, `--abbrev-commit`, `--oneline`, `--format`, `[--] <path>...`

Walks the commit graph exactly like git's revision machinery (date-ordered walk, gradual UNINTERESTING marking, path-limited history simplification with parent rewriting, topological sorting) and prints matching commits.

<details>
<summary>Options as printed by <code>rgit rev-list -h</code></summary>

```text
usage: rgit rev-list [<options>] <commit>... [--] [<path>...]
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit rev-list --count HEAD
rgit rev-list main..feature
rgit rev-list --oneline -n 2 HEAD
```


### `rgit ls-files`

List files in the index and working tree.

**Arguments and flags:** `-c`, `-m`, `-d`, `-o`, `-i`, `--exclude-standard`, `-s`, `-u`, `-t`, `-z`, `[<pathspec>...]`

Lists tracked index entries, modified/deleted tracked files, and untracked or ignored working-tree files. Conflicted paths are listed once per stage (`-s` shows the stage number, `-t` tags them `M`); `-u` lists only those.

<details>
<summary>Options as printed by <code>rgit ls-files -h</code></summary>

```text
Usage: rgit ls-files [OPTIONS] [PATHSPEC]...

Arguments:
  [PATHSPEC]...  

Options:
  -c, --cached            
  -d, --deleted           
  -m, --modified          
  -o, --others            
  -i, --ignored           
      --exclude-standard  
  -s, --stage             
  -u, --unmerged          
  -t                      
  -z                      
  -h, --help              Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit ls-files
rgit ls-files -s
echo n > untracked.txt
rgit ls-files -o --exclude-standard
```


### `rgit check-ignore`

Tell which paths are ignored, and by which rule.

**Arguments and flags:** `-q`, `-v`, `-n`, `--no-index`, `--stdin`, `-z`, `<pathname>...`

Reports which paths are excluded by `.gitignore` rules; `-v` shows the deciding rule as `<source>:<line>:<pattern>`. Exits 0 if any path is ignored, 1 if none, 128 on usage errors.

<details>
<summary>Options as printed by <code>rgit check-ignore -h</code></summary>

```text
Usage: rgit check-ignore [OPTIONS] [PATHNAME]...

Arguments:
  [PATHNAME]...  

Options:
  -q, --quiet         
  -v, --verbose       
  -n, --non-matching  
      --no-index      
      --stdin         
  -z                  
  -h, --help          Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo '*.log' > .gitignore
rgit check-ignore -v debug.log
```


### `rgit check-ref-format`

Check that a reference name is valid.

**Arguments and flags:** `--normalize|--print`, `--[no-]allow-onelevel`, `--refspec-pattern`, `<refname>`; `--branch <name>`

Validates a ref name against git's naming rules, exiting 0 if valid and 1 if not; `--normalize` collapses slashes and prints the result, and `--branch` checks and prints a branch name (exit 128 if invalid). Works outside a repository.

<details>
<summary>Options as printed by <code>rgit check-ref-format -h</code></summary>

```text
Usage: rgit check-ref-format [OPTIONS] [REFNAME]

Arguments:
  [REFNAME]  

Options:
      --normalize            [alias: --print]
      --allow-onelevel       
      --no-allow-onelevel    
      --refspec-pattern      
      --branch <BRANCHNAME>  
  -h, --help                 Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit check-ref-format refs/heads/ok
rgit check-ref-format --branch main
```


### `rgit show-ref`

List references and their commits.

**Arguments and flags:** `[<pattern>...]`, `--head`, `--heads`, `--tags`, `-d|--dereference`, `-s|--hash[=<n>]`, `--abbrev[=<n>]`, `-q`; `--verify <ref>...`; `--exists <ref>`; `--exclude-existing[=<pattern>]`

Lists loose and packed refs with their objects, matching patterns against whole trailing path components; `-d` adds peeled `^{}` lines for annotated tags, `--verify` checks exact ref names, `--exists` checks one ref without resolving it (exit 2 if missing), and `--exclude-existing` filters refs from stdin down to those missing locally.

<details>
<summary>Options as printed by <code>rgit show-ref -h</code></summary>

```text
error: unknown switch `h'
usage: rgit show-ref [--head] [-d | --dereference]
                     [-s | --hash[=<n>]] [--abbrev[=<n>]] [--tags]
                     [--heads] [--] [<pattern>...]
   or: rgit show-ref --verify [-q | --quiet] [-d | --dereference]
                     [-s | --hash[=<n>]] [--abbrev[=<n>]]
                     [--] [<ref>...]
   or: rgit show-ref --exclude-existing[=<pattern>]
   or: rgit show-ref --exists <ref>
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit show-ref
rgit show-ref --heads
rgit show-ref --verify refs/heads/main
```


### `rgit symbolic-ref`

Read, set and delete symbolic references.

**Arguments and flags:** `<name>`, `-q|--quiet`, `--short`, `--[no-]recurse`; `[-m <reason>] <name> <ref>`; `-d|--delete <name>`

Reads, sets and deletes symbolic refs such as `HEAD` or `refs/remotes/origin/HEAD`. With one name it prints the ref it points at (following chains unless `--no-recurse`, shortened with `--short`; unborn targets still print), exiting 1 quietly with `-q` when the ref is not symbolic. With a target it rewrites the ref's own file (HEAD must stay inside `refs/`) and logs the move in its reflog, with `-m` as the message. `--delete` removes a symbolic ref (never HEAD) and its reflog.

<details>
<summary>Options as printed by <code>rgit symbolic-ref -h</code></summary>

```text
usage: rgit symbolic-ref [-m <reason>] <name> <ref>
   or: rgit symbolic-ref [-q] [--short] [--no-recurse] <name>
   or: rgit symbolic-ref --delete [-q] <name>

    -q, --[no-]quiet      suppress error message for non-symbolic (detached) refs
    -d, --[no-]delete     delete symbolic ref
    --[no-]short          shorten ref output
    --[no-]recurse        recursively dereference (default)
    -m <reason>           reason of the update
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit symbolic-ref HEAD
rgit symbolic-ref --short HEAD
```


### `rgit update-ref`

Set or delete a reference safely.

**Arguments and flags:** `[-m <reason>] [--no-deref] [--create-reflog] <ref> <new-oid> [<old-oid>]`; `-d <ref> [<old-oid>]`; `--stdin [-z]`

Safely sets or deletes any ref. Values go through revision parsing, an `<old-oid>` makes the change conditional on the current value (empty = must not exist), and symbolic refs like HEAD are followed unless `--no-deref`. Objects must exist and branches only take commits; new refs may not clash with existing ones. Changes are logged with the `-m` message (also in HEAD's reflog for the current branch), and deletions also drop the `packed-refs` entry and the reflog. `--stdin` runs an atomic transaction of `update`/`create`/`delete`/`verify`/`option no-deref` commands, with `start`/`prepare`/`commit`/`abort` for interactive control (C-quoted names, or NUL-separated input with `-z`).

<details>
<summary>Options as printed by <code>rgit update-ref -h</code></summary>

```text
usage: rgit update-ref [<options>] -d <refname> [<old-val>]
   or: rgit update-ref [<options>]    <refname> <new-val> [<old-val>]
   or: rgit update-ref [<options>] --stdin [-z]

    -m <reason>           reason of the update
    -d                    delete the reference
    --no-deref            update <refname> not the one it points to
    --deref               opposite of --no-deref
    -z                    stdin has NUL-terminated arguments
    --[no-]stdin          read updates from stdin
    --[no-]create-reflog  create a reflog
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit update-ref refs/heads/saved HEAD
rgit update-ref -d refs/heads/saved
```


### `rgit pack-refs`

Move loose references into packed-refs.

**Arguments and flags:** `--all`, `--no-prune`, `--include <pattern>`, `--exclude <pattern>`

Moves loose refs into `.git/packed-refs` (git's format, with `^<peeled>` lines for annotated tags). By default only tags are packed; `--all` packs every ref, and `--include`/`--exclude` select refs by glob (excludes win). Symbolic, per-worktree and broken refs stay loose; packed loose files are removed afterwards (keeping their reflogs) unless `--no-prune`.

<details>
<summary>Options as printed by <code>rgit pack-refs -h</code></summary>

```text
usage: rgit pack-refs [--all] [--no-prune] [--include <pattern>] [--exclude <pattern>]

    --[no-]all            pack everything
    --[no-]prune          prune loose refs (default)
    --[no-]include <pattern>
                          references to include
    --[no-]exclude <pattern>
                          references to exclude
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit pack-refs --all
```


### `rgit for-each-ref`

List references with a custom output format.

**Arguments and flags:** `[<pattern>...]`, `--format=<format>`, `--sort=<key>` (`-`, `version:`), `--count=<n>`, `--exclude=<pattern>`, `--points-at=<object>`, `--merged`/`--no-merged [<commit>]`, `--contains`/`--no-contains [<commit>]`, `--ignore-case`, `--omit-empty`, `--stdin`, `-s|--shell`, `-p|--perl`, `--python`, `--tcl`, `--color[=<when>]`

Lists refs through `%(atom)` formats with git's atoms — `refname`/`symref` (`:short`, `:lstrip=`, `:rstrip=`), `objectname`/`tree`/`parent` (`:short[=N]`), object type/size, people and dates (all date modes, `format:`), `subject`/`body`/`contents`/`trailers`, `upstream`/`push` (`:track`, `:trackshort`, `:remotename`, `:remoteref`), `HEAD`, `flag`, `worktreepath`, `describe`, `ahead-behind:`, `color:`, `align`, `if`/`then`/`else`/`end`, and `*` to dereference tags. Mailmap options are accepted but not applied, and signatures are reported as unsigned.

<details>
<summary>Options as printed by <code>rgit for-each-ref -h</code></summary>

```text
usage: rgit for-each-ref [<options>] [<pattern>]
   or: rgit for-each-ref [--points-at <object>]
   or: rgit for-each-ref [--merged [<commit>]] [--no-merged [<commit>]]
   or: rgit for-each-ref [--contains [<commit>]] [--no-contains [<commit>]]

    -s, --[no-]shell      quote placeholders suitably for shells
    -p, --[no-]perl       quote placeholders suitably for perl
    --[no-]python         quote placeholders suitably for python
    --[no-]tcl            quote placeholders suitably for Tcl
    --[no-]omit-empty     do not output a newline after empty formatted refs

    --[no-]count <n>      show only <n> matched refs
    --[no-]format <format>
                          format to use for the output
    --[no-]color[=<when>] respect format colors
    --[no-]exclude <pattern>
                          exclude refs which match pattern
    --[no-]sort <key>     field name to sort on
    --[no-]points-at <object>
                          print only refs which points at the given object
    --merged <commit>     print only refs that are merged
    --no-merged <commit>  print only refs that are not merged
    --contains <commit>   print only refs which contain the commit
    --no-contains <commit>
                          print only refs which don't contain the commit
    --[no-]ignore-case    sorting and filtering are case insensitive
    --[no-]stdin          read reference patterns from stdin
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit for-each-ref --format='%(refname:short) %(objectname:short)' refs/heads
rgit for-each-ref --sort=-committerdate --count=3
```


### `rgit refs`

Umbrella command for listing, checking and packing references.

**Arguments and flags:** `list ...`, `exists <ref>`, `verify [--strict] [--verbose]`, `optimize ...`

git 2.46+ umbrella command: `list` = `for-each-ref`, `exists` = `show-ref --exists`, `optimize` = `pack-refs`, and `verify` checks loose ref names/contents and `packed-refs` for consistency.

<details>
<summary>Options as printed by <code>rgit refs -h</code></summary>

```text
usage: rgit refs list [<for-each-ref options>] [<pattern>...]
   or: rgit refs exists <ref>
   or: rgit refs verify [--strict] [--verbose]
   or: rgit refs optimize [<pack-refs options>]
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit refs list
rgit refs exists refs/heads/main
```


### `rgit replace`

Make one object stand in for another.

**Arguments and flags:** `[-f] <object> <replacement>`, `--graft <commit> [<parent>...]`, `--edit <object>` (`--raw`), `-d <object>...`, `-l [<pattern>]` (`--format=short|medium|long`)

Manages `refs/replace/` so that reads of an object return its replacement; `fsck`, `gc`, `prune`, `count-objects`, `bundle`, `push`, `fetch`, `clone` and `pull` ignore replacements, as does everything when `GIT_NO_REPLACE_OBJECTS` is set.

<details>
<summary>Options as printed by <code>rgit replace -h</code></summary>

```text
usage: rgit replace [-f] <object> <replacement>
   or: rgit replace [-f] --edit <object>
   or: rgit replace [-f] --graft <commit> [<parent>...]
   or: rgit replace -d <object>...
   or: rgit replace [--format=<format>] [-l [<pattern>]]

    -l, --list            list replace refs
    -d, --delete          delete replace refs
    -e, --edit            edit existing object
    -g, --graft           change a commit's parents
    -f, --force           replace the ref if it exists
    --raw                 do not pretty-print contents for --edit
    --format <format>     use this format
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit replace HEAD~1 feature
rgit replace -l
```


### `rgit notes`

Attach notes to commits.

**Arguments and flags:** `--ref <ref>`; `list`, `add`, `append`, `edit`, `copy`, `show`, `remove`, `prune`, `merge -s <strategy>`, `get-ref`

Attaches notes to commits under `refs/notes/commits`, in git's tree layout. `log` and `show` print them in a `Notes:` block and as `%N` (`--notes` / `--no-notes`). Notes merge conflicts can only be resolved with a strategy, not by hand.

<details>
<summary>Options as printed by <code>rgit notes -h</code></summary>

```text
usage: rgit notes [--ref <notes-ref>] [list [<object>]]
   or: rgit notes [--ref <notes-ref>] add [-f] [--allow-empty] [-F <file> | -m <msg> | (-c | -C) <object>] [<object>]
   or: rgit notes [--ref <notes-ref>] copy [-f] <from-object> [<to-object>]
   or: rgit notes [--ref <notes-ref>] append [--allow-empty] [-F <file> | -m <msg> | (-c | -C) <object>] [<object>]
   or: rgit notes [--ref <notes-ref>] edit [--allow-empty] [<object>]
   or: rgit notes [--ref <notes-ref>] show [<object>]
   or: rgit notes [--ref <notes-ref>] merge [-s <strategy>] <notes-ref>
   or: rgit notes [--ref <notes-ref>] remove [--ignore-missing] [<object>...]
   or: rgit notes [--ref <notes-ref>] prune [-n] [-v]
   or: rgit notes [--ref <notes-ref>] get-ref
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit notes add -m "reviewed" HEAD
rgit notes show HEAD
rgit log -1
```


### `rgit update-index`

Edit the index directly.

**Arguments and flags:** `--add`, `--remove`, `--force-remove`, `--replace`, `--cacheinfo <mode>,<object>,<path>`, `--chmod=(+|-)x`, `--refresh`, `-q`, `--ignore-missing`, `--info-only`, `--index-info`, `--stdin`, `-z`, `-g|--again`, `--verbose`, `<file>...`

Low-level index editing with git's in-order option handling. `--assume-unchanged`, `--skip-worktree` and stage numbers are refused (no entry flags or stages in rgit's index).

<details>
<summary>Options as printed by <code>rgit update-index -h</code></summary>

```text
usage: rgit update-index [<options>] [--] [<file>...]

    -q                    continue refresh even when index needs update
    --[no-]add            do not ignore new files
    --[no-]replace        let files replace directories and vice-versa
    --[no-]remove         notice files missing from worktree
    --refresh             refresh stat information
    --really-refresh      like --refresh
    --cacheinfo <mode>,<object>,<path>
                          add the specified entry to the index
    --chmod (+|-)x        override the executable bit of the listed files
    --[no-]info-only      add to index only; do not add content to object database
    --[no-]force-remove   remove named paths even if present in worktree
    -z                    with --stdin: input lines are terminated by null bytes
    --stdin               read list of paths to be updated from standard input
    --index-info          add entries from standard input to the index
    -g, --again           only update entries that differ from HEAD
    --[no-]ignore-missing ignore files missing from worktree
    --[no-]verbose        report actions to standard output
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo n > new.txt
rgit update-index --add new.txt
rgit ls-files -s
```


### `rgit checkout-index`

Copy index entries to the working tree.

**Arguments and flags:** `-a`, `-f`, `-q`, `-n`, `-u`, `--prefix=<string>`, `--temp`, `--stage=<n>|all`, `--stdin`, `-z`, `<file>...`

Copies index entries to the working tree, a prefix directory, or temporary files.

<details>
<summary>Options as printed by <code>rgit checkout-index -h</code></summary>

```text
usage: rgit checkout-index [<options>] [--] [<file>...]

    -a, --[no-]all        check out all files in the index
    -f, --[no-]force      force overwrite of existing files
    -q, --[no-]quiet      no warning for existing files and files not in index
    -n, --no-create       don't checkout new files
    --create              opposite of --no-create
    -u, --[no-]index      update stat information in the index file
    -z                    paths are separated with NUL character
    --[no-]stdin          read list of paths from the standard input
    --[no-]temp           write the content to temporary files
    --[no-]prefix <string>
                          when creating files, prepend <string>
    --stage (1|2|3|all)   copy out the files from named stage
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rm a.txt
rgit checkout-index a.txt
rgit checkout-index -a -f
```


### `rgit read-tree`

Read trees into the index.

**Arguments and flags:** `<tree-ish>...`, `--empty`, `--prefix=<dir>/`, `-m`, `--reset`, `-u`, `-i`, `-n`, `--trivial`, `--aggressive`

Reads trees into the index: plain replace, bind under a prefix, one-way merge, two-way merge (branch switch, carrying local index changes) and trivial three-way merge, optionally updating the working tree. Paths that would need conflict stages are refused.

<details>
<summary>Options as printed by <code>rgit read-tree -h</code></summary>

```text
usage: rgit read-tree [(-m [--trivial] [--aggressive] | --reset | --prefix=<prefix>)
                      [-u | -i]] [-n] (--empty | <tree-ish1> [<tree-ish2> [<tree-ish3>]])

    -m                    perform a merge in addition to a read
    --reset               same as -m, but discard local changes
    --prefix <subdirectory>/
                          read the tree into the index under <subdirectory>/
    -u                    update working tree with merge result
    -i                    don't check the working tree after merging
    -n, --dry-run         don't update the index or the work tree
    --trivial             3-way merge if no file level merging required
    --aggressive          3-way merge in presence of adds and removes
    --empty               only empty the index
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit read-tree HEAD~1
rgit read-tree HEAD
```


### `rgit diff-tree`

Raw diff between two trees.

**Syntax:** `rgit diff-tree [<options>] <tree-ish> [<tree-ish>] [<path>...]`

The raw plumbing diff between two trees (or a commit and its parent). `-r` recurses into subfolders, `-t` also shows the tree entries themselves, `--root` lets a root commit be compared with the empty tree, `-m` shows merges against each parent, `--stdin` reads commit IDs from standard input, `-v` / `--pretty` print the commit header, `--no-commit-id` omits it. Output formats: raw (`:<modes> <hashes> <status> TAB <path>`, the default), `-p` (patch), `--name-only`, `--name-status`, `--stat`, `--shortstat`, `--numstat`, `--summary`, `-z`, `--abbrev[=<n>]`, `-R` (reverse), `--quiet`, `--exit-code`, and rename/copy detection with `-M` / `-C`. For unmerged paths, `diff-index --cached` shows one `U` entry each; `diff-files` shows `U` plus the comparison of stage 2 (`-1`/`-3` choose stage 1/3) with the working tree, and with `-p` (or `--cc`) a combined diff for paths with both sides.

<details>
<summary>Options as printed by <code>rgit diff-tree -h</code></summary>

```text
usage: rgit diff-tree [--stdin] [-m] [-s] [-v] [--no-commit-id] [--pretty]
              [-t] [-r] [--root] [<common-diff-options>] <tree-ish> [<tree-ish>] [<path>...]

  -r            diff recursively
  -t            show tree entries as well
  --root        include the initial commit as diff against /dev/null

common diff options:
  -z            output diff-raw with lines terminated with NUL.
  -p            output patch format.
  -u            synonym for -p.
  --raw         output diff-raw (the default).
  --name-only   show only names of changed files.
  --name-status show names and status of changed files.
  --stat        show a diffstat.
  --shortstat   show only the last line of the diffstat.
  --numstat     show added and deleted line counts per file.
  --summary     show creations, deletions and mode changes.
  -s            no diff output.
  --abbrev[=<n>] abbreviate object names in diff-raw.
  -R            swap input file pairs.
  --exit-code   exit with 1 if there were differences.
  --quiet       no output, implies --exit-code.
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit diff-tree -r HEAD~1 HEAD
rgit diff-tree -r --name-status HEAD
```


### `rgit diff-index`

Raw diff between a tree and the index or working tree.

**Syntax:** `rgit diff-index [--cached] [-m] <tree-ish> [<path>...]`

The raw plumbing diff between a tree and the index (`--cached`) or the working tree. `-m` treats a missing working file as unchanged. Output formats: raw (`:<modes> <hashes> <status> TAB <path>`, the default), `-p` (patch), `--name-only`, `--name-status`, `--stat`, `--shortstat`, `--numstat`, `--summary`, `-z`, `--abbrev[=<n>]`, `-R` (reverse), `--quiet`, `--exit-code`, and rename/copy detection with `-M` / `-C`. For unmerged paths, `diff-index --cached` shows one `U` entry each; `diff-files` shows `U` plus the comparison of stage 2 (`-1`/`-3` choose stage 1/3) with the working tree, and with `-p` (or `--cc`) a combined diff for paths with both sides.

<details>
<summary>Options as printed by <code>rgit diff-index -h</code></summary>

```text
usage: rgit diff-index [-m] [--cached] [<common-diff-options>] <tree-ish> [<path>...]

common diff options:
  -z            output diff-raw with lines terminated with NUL.
  -p            output patch format.
  -u            synonym for -p.
  --raw         output diff-raw (the default).
  --name-only   show only names of changed files.
  --name-status show names and status of changed files.
  --stat        show a diffstat.
  --shortstat   show only the last line of the diffstat.
  --numstat     show added and deleted line counts per file.
  --summary     show creations, deletions and mode changes.
  -s            no diff output.
  --abbrev[=<n>] abbreviate object names in diff-raw.
  -R            swap input file pairs.
  --exit-code   exit with 1 if there were differences.
  --quiet       no output, implies --exit-code.
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo more >> a.txt
rgit diff-index HEAD
```


### `rgit diff-files`

Raw diff between the index and the working tree.

**Syntax:** `rgit diff-files [<path>...]`

The raw plumbing diff between the index and the working tree. Output formats: raw (`:<modes> <hashes> <status> TAB <path>`, the default), `-p` (patch), `--name-only`, `--name-status`, `--stat`, `--shortstat`, `--numstat`, `--summary`, `-z`, `--abbrev[=<n>]`, `-R` (reverse), `--quiet`, `--exit-code`, and rename/copy detection with `-M` / `-C`. For unmerged paths, `diff-index --cached` shows one `U` entry each; `diff-files` shows `U` plus the comparison of stage 2 (`-1`/`-3` choose stage 1/3) with the working tree, and with `-p` (or `--cc`) a combined diff for paths with both sides.

<details>
<summary>Options as printed by <code>rgit diff-files -h</code></summary>

```text
usage: rgit diff-files [-q] [<common-diff-options>] [<path>...]

common diff options:
  -z            output diff-raw with lines terminated with NUL.
  -p            output patch format.
  -u            synonym for -p.
  --raw         output diff-raw (the default).
  --name-only   show only names of changed files.
  --name-status show names and status of changed files.
  --stat        show a diffstat.
  --shortstat   show only the last line of the diffstat.
  --numstat     show added and deleted line counts per file.
  --summary     show creations, deletions and mode changes.
  -s            no diff output.
  --abbrev[=<n>] abbreviate object names in diff-raw.
  -R            swap input file pairs.
  --exit-code   exit with 1 if there were differences.
  --quiet       no output, implies --exit-code.
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo more >> a.txt
rgit diff-files
```


### `rgit stripspace`

Clean up whitespace in text from standard input.

**Arguments and flags:** `-s|--strip-comments`, `-c|--comment-lines`

git's message cleanup filter for standard input: trims trailing whitespace, collapses and trims empty lines; `-s` drops comment lines, `-c` turns every line into a comment (`core.commentChar`).

<details>
<summary>Options as printed by <code>rgit stripspace -h</code></summary>

```text
usage: rgit stripspace [-s | --strip-comments]
   or: rgit stripspace [-c | --comment-lines]

    -s, --strip-comments  skip and remove all lines starting with comment character
    -c, --comment-lines   prepend comment character and space to each line
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
printf 'a  \n\n\n b\n' | rgit stripspace
```


### `rgit column`

Arrange lines from standard input in columns.

**Arguments and flags:** `--mode[=<style>]`, `--raw-mode=<n>`, `--width=<n>`, `--padding=<n>`, `--indent=<string>`, `--nl=<string>`, `--command=<name>`

Lays lines from standard input out in columns (`column`/`row`/`plain`, `dense`), honouring `column.ui` and `column.<name>`.

<details>
<summary>Options as printed by <code>rgit column -h</code></summary>

```text
usage: rgit column [<options>]

    --[no-]command <name> lookup config vars
    --[no-]mode[=<style>] layout to use
    --[no-]raw-mode <n>   layout to use
    --[no-]width <n>      maximum width
    --[no-]indent <string>
                          padding space on left border
    --[no-]nl <string>    padding space on right border
    --[no-]padding <n>    padding space between columns
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
printf 'a\nb\nc\nd\n' | rgit column --mode=column --width=20
```


## 11. Storage and maintenance

Packfiles, indexes and housekeeping.


### `rgit repack`

Pack loose objects into a packfile.

**Arguments and flags:** `-a`, `-A`, `-d`, `--window=<n>`, `--depth=<n>`

Packs loose objects (or with `-a` everything reachable) into a new packfile with delta compression; `-d` removes redundant loose objects and older packs.

<details>
<summary>Options as printed by <code>rgit repack -h</code></summary>

```text
usage: rgit repack [-a] [-A] [-d] [-f] [-F] [-l] [-n] [-q] [--window=<n>] [--depth=<n>]
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit repack -a -d
```


### `rgit pack-objects`

Create a packfile from objects or revisions.

**Arguments and flags:** `--revs`, `--all`, `--unpacked|--incremental`, `--non-empty`, `--window=<n>`, `--depth=<n>`, `--progress`, `--stdout | <base-name>`

Builds a pack from object names or revisions on standard input.

<details>
<summary>Options as printed by <code>rgit pack-objects -h</code></summary>

```text
usage: rgit pack-objects [-q | --progress] [--revs] [--all] [--unpacked | --incremental] [--non-empty]
                         [--window=<n>] [--depth=<n>] (--stdout | <base-name>) < <object-list>
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
echo HEAD | rgit pack-objects --revs --stdout > out.pack
```


### `rgit unpack-objects`

Explode a packfile into loose objects.

**Arguments and flags:** `-n`, `-r`

Explodes a pack from standard input into loose objects.

<details>
<summary>Options as printed by <code>rgit unpack-objects -h</code></summary>

```text
usage: rgit unpack-objects [-n] [-q] [-r] [--strict]
```

</details>


### `rgit index-pack`

Build an index for a packfile.

**Arguments and flags:** `<pack-file>`, `-o <index>`, `--stdin`, `--fix-thin`, `--keep`, `--verify`

Builds the `.idx` for a pack (byte-identical to git's), or stores a pack read from standard input.

<details>
<summary>Options as printed by <code>rgit index-pack -h</code></summary>

```text
usage: rgit index-pack [-v] [-o <index-file>] [--keep | --keep=<msg>] [--verify] [--strict] (<pack-file> | --stdin [--fix-thin] [<pack-file>])
```

</details>


### `rgit verify-pack`

Check packfiles and list their objects.

**Arguments and flags:** `-v`, `-s`

Checks packs and lists their objects and delta chains.

<details>
<summary>Options as printed by <code>rgit verify-pack -h</code></summary>

```text
usage: rgit verify-pack [-v | --verbose] [-s | --stat-only] [--] <pack>.idx...

    -v, --[no-]verbose    verbose
    -s, --[no-]stat-only  show statistics only
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit repack -a -d
rgit verify-pack -v .git/objects/pack/pack-*.idx
```


### `rgit show-index`

Print the contents of a pack index.

**Syntax:** `rgit show-index < file.idx`

Reads a pack index (`.idx`) from standard input and lists its objects with their offsets.

<details>
<summary>Options as printed by <code>rgit show-index -h</code></summary>

```text
usage: rgit show-index
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit repack -a -d
rgit show-index < .git/objects/pack/pack-*.idx
```


### `rgit prune-packed`

Remove loose objects that are already in a pack.

**Arguments and flags:** `-n`

Removes loose objects that are also in a pack.

<details>
<summary>Options as printed by <code>rgit prune-packed -h</code></summary>

```text
usage: rgit prune-packed [-n | --dry-run] [-q | --quiet]

    -n, --[no-]dry-run    dry run
    -q, --[no-]quiet      be quiet
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit repack -a
rgit prune-packed -n
```


### `rgit commit-graph`

Write and verify the commit-graph file.

**Arguments and flags:** `write [--reachable | --stdin-commits | --stdin-packs] [--append]`, `verify`

Writes and checks git's commit-graph file (identical to git's output; not used for rgit's own walks).

<details>
<summary>Options as printed by <code>rgit commit-graph -h</code></summary>

```text
usage: rgit commit-graph verify
   or: rgit commit-graph write [--append] [--reachable | --stdin-packs | --stdin-commits]
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit commit-graph write --reachable
rgit commit-graph verify
```


### `rgit multi-pack-index`

Write and verify the multi-pack-index.

**Arguments and flags:** `write [--preferred-pack=<pack>]`, `verify`, `expire`

Writes and checks git's multi-pack-index (identical to git's output).

<details>
<summary>Options as printed by <code>rgit multi-pack-index -h</code></summary>

```text
usage: rgit multi-pack-index write [--preferred-pack=<pack>]
   or: rgit multi-pack-index verify
   or: rgit multi-pack-index expire
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit repack -a -d
rgit multi-pack-index write
rgit multi-pack-index verify
```


### `rgit count-objects`

Count loose objects and disk usage.

**Arguments and flags:** `-v|--verbose`, `-H|--human-readable`

Reports loose object count and disk usage; verbose mode adds pack, prune-packable, and garbage statistics.

<details>
<summary>Options as printed by <code>rgit count-objects -h</code></summary>

```text
Usage: rgit count-objects [OPTIONS]

Options:
  -v, --verbose         
  -H, --human-readable  
  -h, --help            Print help
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit count-objects -v
rgit count-objects -vH
```


### `rgit maintenance`

Run housekeeping tasks.

**Arguments and flags:** `run [--auto] [--quiet] [--task=<task>]`, `register`, `unregister [--force]`

Runs upkeep tasks (`gc`, `pack-refs`; the packfile and commit-graph tasks are accepted but skipped) and manages the global `maintenance.repo` list. No `start` / `stop` scheduler.

<details>
<summary>Options as printed by <code>rgit maintenance -h</code></summary>

```text
usage: rgit maintenance run [--auto] [--[no-]quiet] [--task=<task>]
   or: rgit maintenance register
   or: rgit maintenance unregister [--force]
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit maintenance run
```


### `rgit for-each-repo`

Run a command in every repository from a config list.

**Arguments and flags:** `--config=<key>`, `--keep-going`, `<arguments>`

Runs `rgit <arguments>` in every repository listed under a multi-valued config key.

<details>
<summary>Options as printed by <code>rgit for-each-repo -h</code></summary>

```text
usage: rgit for-each-repo --config=<config> [--keep-going] [--] <arguments>

    --config <config>     config key storing a list of repository paths
    --keep-going          keep going even if command fails in a repository
```

</details>


### `rgit hook`

Run a repository hook script by hand.

**Arguments and flags:** run `--ignore-missing`, `--to-stdin=<path>`, `<hook-name> [-- <hook-args>]`

Runs a hook script from `.git/hooks` or `core.hooksPath` and exits with its status. rgit's own commands do not fire hooks yet.

<details>
<summary>Options as printed by <code>rgit hook -h</code></summary>

```text
usage: rgit hook run [--ignore-missing] [--to-stdin=<path>] <hook-name> [-- <hook-args>]

    --ignore-missing      silently ignore missing requested <hook-name>
    --to-stdin <path>     file to read into hooks' stdin
```

</details>

**Examples** (run in a repository that has a `main` branch with two commits and a `feature` branch with one more)

```bash
rgit hook run --ignore-missing pre-commit
```


---

# Everyday recipes

## Undo things

```bash
rgit restore file.txt                  # throw away unstaged edits to a file
rgit restore --staged file.txt         # unstage a file, keep the edits
rgit commit --amend -m "Better text"   # fix the last commit message
rgit reset --soft HEAD~1               # undo the last commit, keep changes staged
rgit reset --hard HEAD~1               # undo the last commit AND the changes
rgit revert HEAD                       # new commit that undoes the last one (safe when shared)
rgit reflog                            # find where you were; then: rgit reset --hard HEAD@{1}
```

## Work on something else for a moment

```bash
rgit stash                             # set uncommitted work aside
rgit switch other-branch
# ...fix something, commit...
rgit switch feature                    # back to the branch you were on
rgit stash pop                         # bring the work back
# rgit-only, per branch and with untracked files: rgit wip "what I was doing" ... rgit wip --restore
```

## Resolve a merge conflict

```bash
rgit merge feature                     # reports CONFLICT in file.txt
rgit conflicts                         # rgit-only: what is left and who changed what
rgit conflicts --explain file.txt      # each conflict side by side
# edit file.txt, remove the <<<<<<< ======= >>>>>>> lines
rgit add file.txt
rgit merge --continue                  # or: rgit commit (opens your editor for the message)
# changed your mind: rgit merge --abort
```

## Share work through a local "remote"

```bash
rgit clone ../project my-copy          # copy a repository
cd my-copy
rgit switch -c topic
# ...commit...
rgit push -u origin topic              # upload and remember the upstream
rgit fetch                             # later: see what changed there
rgit pull                              # fetch + merge into the current branch
```

## Send a change as a patch

```bash
rgit format-patch -1                   # writes 0001-your-subject.patch
rgit apply --check 0001-*.patch        # test it on another copy
rgit am 0001-*.patch                   # apply it as a commit, keeping the author
```

## Find which commit broke something

```bash
rgit bisect start
rgit bisect bad                        # the current commit is broken
rgit bisect good v1.0                  # this older one worked
# rgit checks out the middle; test, then say: rgit bisect good  |  rgit bisect bad
rgit bisect reset                      # go back when done
```

## Keep the repository healthy

```bash
rgit fsck                              # verify objects and refs
rgit count-objects -vH                 # how much space loose objects use
rgit gc                                # expire old reflog entries, drop unreachable objects
rgit repack -a -d                      # pack loose objects into a packfile
```

---

# Known differences and limits

rgit aims for git's behaviour and output, but some things are different or missing:

- **Transports.** Only local paths and GitHub (REST API). No SSH, no smart HTTP git protocol.
- **Bundles.** `bundle create`, `verify`, `list-heads` and `unbundle` work; `clone` *from* a bundle file does not yet.
- **`stash show`** with no flag prints the full patch; git prints only a summary there and the patch with `-p`. `-p` is accepted.
- **`reflog <ref>`** needs the word `show` (`rgit reflog show main`); `reflog -n <count>` works on its own.
- **`switch`** accepts a branch name (or a full commit hash with `--detach`); revision expressions such as `HEAD~1` are not accepted there. Use `checkout` or `rev-parse`.
- **`init`** takes no directory argument: create the folder and enter it first.
- **`cat-file`** without `-p`, `-t`, `-s` or `-e` prints nothing (git prints a usage error).
- **`revert --no-edit`** and **`merge --no-edit`** are accepted for compatibility. Reverts and clean merges never open an editor; concluding a conflicted merge (`merge --continue` or `commit`) does, so set `RGIT_EDITOR` (for example `RGIT_EDITOR=true` to accept the default message).
- **`GIT_EDITOR`** is not read; use `RGIT_EDITOR`, `core.editor`, `VISUAL` or `EDITOR`.
- **Hooks.** `hook run` can run a hook by hand, but rgit's own commands do not fire hooks yet.
- **`rerere`** is not called automatically by `merge`, `rebase` or `commit`; run it by hand.
- **Merge strategies.** `ort` (and `recursive`, which runs ort) and `ours`; `-X ours|theirs`. No octopus merges, no `resolve`/`octopus`/`subtree` strategies.
- **`merge-tree`** does not detect renames.
- **Renames** are detected by default in `diff`, `log`, `show`, `whatchanged` and `status`; opt-in (`-M`, `-C`) for the plumbing diffs and `fast-export`.
- **`remote rename`** moves loose remote-tracking branches; ones already packed into `packed-refs` keep the old remote name.
- **`rev-parse --abbrev-ref origin/HEAD`** prints `main`; git prints `origin/main`.
- **Piping into a program that closes early** (for example `rgit status | head -3`) can end with a Rust "Broken pipe" panic message instead of exiting quietly. Output before it is correct.
- **Repositories.** Tested on small scenario repositories. Very large repositories, partial or shallow clones, and unusual index extensions have not been exercised.

# Troubleshooting

| Symptom | What to do |
|---|---|
| `fatal: not a git repository` | You are outside a repository; `cd` into one or run `rgit init`. |
| Commits show author `rgit <rgit@example.com>` | Set your identity: `rgit config --global user.name "..."` and `user.email`. |
| `error: unexpected argument '...' found` (exit 2) | A flag rgit does not have. Check `rgit <command> -h`. |
| `GitHub token not found` | Set `GITHUB_TOKEN`, or `rgit config --global github.token <token>`. |
| `Authentication failed for GitHub` | The token is wrong, expired or lacks the `repo` scope. |
| An editor opens and you did not expect it | Give the message on the command line (`-m`), or set `RGIT_EDITOR` / `core.editor`. |
| A merge stopped halfway | `rgit status`, then `rgit conflicts`; finish with `--continue` or give up with `--abort`. |
| Lost a commit after `reset --hard` | `rgit reflog`, then `rgit reset --hard <hash or HEAD@{n}>`. |
| Output has extra explanatory text | That is narration on a terminal; use `--plain` or `RGIT_VERBOSITY=quiet`. |
| Windows shows `rgit.exe` in usage text | Normal; the program name follows the file name. |
