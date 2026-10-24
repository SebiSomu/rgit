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
  - `rev_list.rs`: Argument parsing, ref collection, a flag-for-flag port of git's revision walk (`limit_list`, `process_parents`, `try_to_simplify_commit`, `rewrite_one`), topological sort, commit filters, and `--format` expansion.
  - `bundle.rs`: Bundle header parsing/writing (v2/v3, prerequisites, refs), revision and ref (`dwim`) resolution, commit/prerequisite selection, object enumeration, packfile writing, and packfile reading with `OFS_DELTA`/`REF_DELTA` resolution (including thin packs).
  - `merge_tree.rs`: Line-level three-way file merge (xdiff zealous level: conflict refinement, joining nearby conflicts, CRLF markers), path-by-path tree merge with git's conflict kinds and messages, file/directory clash handling, and virtual merge bases for criss-cross histories.
  - `cherry.rs`: Upstream lookup from branch config, non-merge commit walks, git-compatible patch IDs (per-file whitespace-stripped hashes summed with carry), and hash abbreviation.
  - `check_ref_format.rs`: Git's ref name rules (component, `..`, `@{`, `.lock`, forbidden bytes, refspec `*`), slash normalization, and branch shorthand checks.
  - `check_ignore.rs`: Source-tracked `.gitignore` rule loading, deciding-rule lookup (including ignored parent directories), and path normalization.
  - `worktree.rs`: Worktree enumeration (HEAD, locked, prunable), path/suffix lookup, `.git/worktrees/<id>/` admin directories, checkout into a new worktree, dirty checks, prune/move, and `list` / `--porcelain` formatting.
- **[`src/index.rs`](file:///c:/rgit-main/src/index.rs)**: Binary staging index parser and serializer (`.git/index`).
- **[`src/refs.rs`](file:///c:/rgit-main/src/refs.rs)**: Reference resolution (`HEAD`, branch heads, symbolic refs, detached HEAD).
- **[`src/repo.rs`](file:///c:/rgit-main/src/repo.rs)**: Repository path resolution: maps `.git/...` paths to the shared `.git` or to a linked worktree's `.git/worktrees/<id>/`.
- **[`src/objects.rs`](file:///c:/rgit-main/src/objects.rs)**: Domain models, shared type aliases, and CLI subcommand enums.
- **[`src/bin/rtest.rs`](file:///c:/rgit-main/src/bin/rtest.rs)**: Isolated sandbox test harness executing `rgit` in `test-sandbox/`.

---

## Features & Supported Commands

### Repository & Staging

- **`init`**: Initializes a new repository with standard `.git/` directory layout.
- **`add <paths...>`**: Hashes file contents into blobs and updates the binary index (`.git/index`).
- **`rm <files...>`** (`--cached`, `-r`, `-f`): Removes files from index and/or working tree.
- **`mv <source> <destination>`** (`-f`): Moves/renames files, staging the change in the index.
- **`status`**: Reports untracked files, working-tree modifications, and staged changes.
- **`restore`** (`--staged`, `--worktree`, `--source`): Restores working tree or index files from a tree or commit.
- **`clean`** (`-f`, `-d`, `-x`, `-X`): Prunes untracked and ignored files with `.gitignore` pattern evaluation.

### History, Inspection & Diffs

- **`commit`** (`-m <message>`): Writes tree and commit objects from the current index.
- **`log`** (`--oneline`): Walks commit parent graphs and displays chronological commit logs.
- **`shortlog`** (`-s|--summary`, `-n|--numbered`, `-e|--email`, `--all`, `[<rev>]`): Summarizes history grouped by author, with commit counts and subject lines.
- **`grep`** (`-i`, `-v`, `-n`, `-c`, `-l|-L`, `-F`, `--cached`, `-e <pattern>`, `<pattern> [<rev>...] [-- <path>...]`): Searches tracked files, the index, or historical revisions using regex or fixed-string patterns.
- **`show [<commit>]`**: Displays commit metadata, message, and unified patch diff.
- **`diff`** (`--staged`, `[<commit> [<commit>]]`): Computes line-by-line Myers diffs between working tree, index, or commits.
- **`apply`** (`--check`, `--stat`, `--numstat`, `--summary`, `--apply`, `--cached`, `--index`, `-R`, `-v`, `-p<n>`, `[<patch>...]`): Applies unified or git-style diffs (including `format-patch` mail) to the working tree and/or index, all-or-nothing, with offset hunk placement, creations, deletions, renames, copies, mode changes, `GIT binary patch` literals and deltas, and CRLF/no-final-newline preservation.
- **`format-patch`** (`-<n>`, `--root`, `--stdout`, `-o <dir>`, `-n|-N`, `--start-number`, `--subject-prefix`, `-k`, `--signature`/`--no-signature`, `<since>|<A..B>`): Exports commits as mailbox patches (`NNNN-subject.patch`) with RFC 2047 headers, diffstat and summary, git-identical hunks (function-name headers, indent heuristic), and binary literals.
- **`merge-tree`** (`[--write-tree]`, `--name-only`, `--[no-]messages`, `-z`, `--merge-base=<tree-ish>`, `--allow-unrelated-histories`, `--explain`, `<branch1> <branch2>`): Merges two commits entirely in memory and writes only the merged tree (conflicted files contain markers), leaving refs, index and working tree untouched. Default output matches git's: tree hash, conflicted stages (`<mode> <oid> <stage>	<path>`), and messages (`Auto-merging`, `CONFLICT (content|add/add|modify/delete|file/directory)`, binary warnings); exits 0 if clean, 1 on conflicts. `--explain` prints a readable report instead: the ours/theirs/base commits (including virtual bases), a per-path table of what each side did and what the merge decided, and a base/ours/theirs preview of every conflicting region. Renames are not detected.
- **`cherry`** (`-v`, `--abbrev[=<n>]`, `[<upstream> [<head> [<limit>]]]`): Lists the commits in `<head>` (default `HEAD`) missing from `<upstream>` (default the branch's upstream), oldest first, marking each `-` if an equivalent change already exists upstream (same patch ID as git computes it, so cherry-picks and whitespace-only differences are recognized) or `+` if it doesn't; merges are ignored and `<limit>` cuts off older commits.
- **`am`** (`[<mbox>...]`, `-k`, `--continue`, `--skip`, `--abort`, `--quit`, `--show-current-patch[=raw|diff]`): Applies a mailbox of patches as commits that keep the original author, date, and message; stops on conflicts with resumable state in `.git/rebase-apply/`.
- **`tag`** (`<name> [<commit>]`, `-d`, `-l`): Creates, lists, or deletes lightweight tags.
- **`describe [<commit>]`**: Finds the most recent reachable tag and formats a descriptive revision identifier.
- **`blame`** (`[-L <start>,<end>] [<rev>] [--] <file>`): Shows line-by-line commit attribution, author info, timestamps, and detects uncommitted lines.
- **`archive`** (`[-f|--format <fmt>]`, `[-l|--list]`, `[--prefix <prefix>]`, `[-o|--output <file>]`, `<tree-ish> [<paths...>]`): Exports files from a tree, commit, tag, or ref into a `tar`, `tar.gz`/`tgz`, or `zip` archive with directory hierarchy and commit timestamps preserved.

### Branching & Navigation

- **`branch`** (`[<name>]`, `-d`, `-D`, `-m`): Lists, creates, renames, and safely deletes branches with ancestor checks.
- **`switch`** (`<branch>`, `-c`, `--detach`, `-f`): Modern branch switching and detached HEAD transitions with worktree safety.
- **`checkout`** (`<branch|commit>`, `-b`, `--detach`, `-f`): Classic branch, commit, and revision switching interface.
- **`bundle`** (`create [-q] [--version=2|3] <file> <rev-list-args>`, `verify [-q] <file>`, `list-heads <file> [<refname>...]`, `unbundle <file> [<refname>...]`): Moves history offline in git's bundle format, so bundles work with real git in both directions (`git clone`/`git fetch` from an rgit bundle, `rgit bundle unbundle` of a git bundle). `create` accepts refs, `^rev`, `A..B`, `A...B`, `--not`, `--all|--branches|--tags|--remotes`, records the prerequisite commits a receiver must already have, and packs every needed object (undeltified, so bundles are larger than git's); `verify` checks prerequisites and describes the bundle; `unbundle` stores the objects as loose objects and prints the refs without updating any.
- **`worktree`** (`add [-b|-B <branch>] [-d] [-f] [--lock [--reason]] <path> [<commit-ish>]`, `list [--porcelain|-v]`, `remove [-f [-f]]`, `move`, `lock [--reason]`, `unlock`, `prune [-n] [-v]`): Manages multiple working trees attached to one repository using git's on-disk layout (`<path>/.git` file → `.git/worktrees/<id>/`), so real git can use them too. Each worktree has its own HEAD, index and in-progress state; objects, refs, config and stashes are shared. A branch can be checked out in only one worktree at a time (`switch`, `checkout` and `branch -d` respect this), `branch -m` updates other worktrees' HEADs, and `gc`/`fsck` treat every worktree's HEAD, index and state files as roots.

### Merging, Replaying & Rebasing

- **`merge <branch>`**: Performs fast-forward or 3-way tree merges with conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`).
- **`merge-base`** (`<commit> <commit>...`, `-a|--all`, `--is-ancestor`): Finds the best common ancestor(s) of commits, or checks ancestry via exit code.
- **`rebase`** (`<upstream>`, `-i`, `--continue`, `--abort`, `--skip`, `--edit-todo`): Full interactive rebase with `pick`, `reword`, `edit`, `squash`, `fixup`, and `drop` instructions.
- **`cherry-pick <commit>`** (`--no-commit`, `--continue`, `--abort`): Replays a single commit onto current HEAD with state recording.
- **`revert <commit>`** (`--no-commit`, `--continue`, `--abort`): Inverts and applies target commit changes.

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

### Configuration & Diagnostics

- **`config`** (`[--global]`, `<key> [<value>]`, `--list`, `--unset`): Reads, writes, lists, and unsets local (`.git/config`) and global (`~/.rgitconfig`) configurations.
- **`bisect`** (`start`, `bad`, `good`, `skip`, `reset`, `log`, `run`): Automated binary search to locate the regression-introducing commit.
- **`banana-split <commit>`** (`--by-dir`, `-n|--dry-run`): Custom tool that splits a commit into atomic per-path or per-directory commits.
- **`gc`** (`--prune <days|now|never>`, `--reflog-expire <days|now|never>`, `-n|--dry-run`): Expires old reflog entries and deletes unreachable loose objects past the prune window, keeping anything reachable from refs, the index, the stash, in-progress operations, or live reflogs.
- **`prune`** (`-n`, `-v`, `--expire <time>`, `[--] <head>...`): Deletes every unreachable loose object (git's default; `--expire` keeps newer ones and whatever they refer to), using the same roots as `gc` (refs, HEAD, index, stash, operations in progress, all reflog entries, every worktree) plus any `<head>` given. Also removes stale `tmp_*` files and reports stray files in `.git/objects`. Silent by default; `-n`/`-v` print `<hash> <type>` per object exactly like git. Refuses to touch a damaged repository or one with packfiles.
- **`fsck`** (`--connectivity-only`, `--no-dangling`, `--unreachable`, `--no-reflogs`): Verifies object integrity and connectivity, validates refs and the index, and reports dangling or unreachable objects.
- **`count-objects`** (`-v|--verbose`, `-H|--human-readable`): Reports loose object count and disk usage; verbose mode adds pack, prune-packable, and garbage statistics.

### Plumbing Commands

- **`hash-object`** (`-w <file>`): Computes object SHA-1 hash and optionally writes compressed object to store.
- **`cat-file`** (`-p <hash>`): Decompresses and displays object type and contents.
- **`write-tree`**: Serializes current staging index into tree objects.
- **`ls-tree`** (`--name-only <tree>`): Inspects tree contents and modes.
- **`ls-files`** (`-c`, `-m`, `-d`, `-o`, `-i`, `--exclude-standard`, `-s`, `-t`, `-z`, `[<pathspec>...]`): Lists tracked index entries, modified/deleted tracked files, and untracked or ignored working-tree files.
- **`check-ignore`** (`-q`, `-v`, `-n`, `--no-index`, `--stdin`, `-z`, `<pathname>...`): Reports which paths are excluded by `.gitignore` rules; `-v` shows the deciding rule as `<source>:<line>:<pattern>`. Exits 0 if any path is ignored, 1 if none, 128 on usage errors.
- **`check-ref-format`** (`--normalize|--print`, `--[no-]allow-onelevel`, `--refspec-pattern`, `<refname>`; `--branch <name>`): Validates a ref name against git's naming rules, exiting 0 if valid and 1 if not; `--normalize` collapses slashes and prints the result, and `--branch` checks and prints a branch name (exit 128 if invalid). Works outside a repository.
- **`commit-tree <tree> [-p <parent>] -m <msg>`**: Directly writes a commit object from tree and parent hashes.
- **`rev-parse`** (`--git-dir`, `--show-toplevel`, `--is-inside-work-tree`, `--is-bare-repository`, `--abbrev-ref`, `--symbolic-full-name`, `--verify [-q]`, `--short[=N]`, `--sq`, `<rev>...`): Resolves revision syntax (`HEAD~2`, `HEAD^`, branches, tags, short hashes, `<ref>@{n}`, `^{type}`) to object hashes or symbolic names, and queries repository properties.
- **`rev-list`** (`<rev>...`, `^rev`, `A..B`, `A...B`, `--not`, `--all|--branches|--tags|--remotes`, `-n|--max-count`, `--skip`, `--reverse`, `--count`, `--topo-order|--date-order`, `--first-parent`, `--merges|--no-merges|--min-parents|--max-parents`, `--since|--until`, `--author|--committer|--grep` (`-i`, `-F`, `--all-match`, `--invert-grep`), `--parents`, `--timestamp`, `--left-right`, `--abbrev-commit`, `--oneline`, `--format`, `[--] <path>...`): Walks the commit graph exactly like git's revision machinery (date-ordered walk, gradual UNINTERESTING marking, path-limited history simplification with parent rewriting, topological sorting) and prints matching commits.

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
