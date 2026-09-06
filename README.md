# rgit

A lightweight, fully functional implementation of Git written in Rust.

`rgit` was built to explore the internal architecture and mechanics of version control systems. It includes a complete implementation of the core Git object model (blobs, trees, commits), index staging, reference tracking (`refs/heads`, `HEAD`), Myers diffing, 3-way merges, stash state stacks, binary-search debugging (`bisect`), and modular crate architecture.

---

## Modular Architecture (v2.0)

`rgit` is structured into clean, decoupled domain modules:

- **[`src/cli.rs`](file:///c:/rgit-main/src/cli.rs)**: Command-line parsing and routing powered by `clap`.
- **[`src/commands/`](file:///c:/rgit-main/src/commands)**: Porcelain & plumbing business logic (split per command namespace: `branch`, `commit`, `diff`, `merge`, `reset`, `stash`, `bisect`, etc.).
- **[`src/helpers/`](file:///c:/rgit-main/src/helpers)**: Core engine utilities (Git object serialization/zlib compression, commit graph BFS traversal, Myers diffing algorithm, `.gitignore` glob matching, working-tree safety checks).
- **[`src/index.rs`](file:///c:/rgit-main/src/index.rs)**: Staging index (`.git/index`) binary file parsing and serialization.
- **[`src/refs.rs`](file:///c:/rgit-main/src/refs.rs)**: Reference resolution (`HEAD`, branch refs, detached HEAD states).
- **[`src/objects.rs`](file:///c:/rgit-main/src/objects.rs)**: Core data models, structs, and CLI subcommand enums.

---

## Features & Supported Commands

### Porcelain Commands (High Level)

#### Repository & Staging
- **`init`**: Initializes `.git/` directory layout.
- **`add <paths...>`**: Hashes file contents into blobs and updates `.git/index`.
- **`rm <files...>`** (`--cached`, `-r`, `-f`): Removes files from index and working tree.
- **`status`**: Reports untracked, modified, and staged changes.
- **`restore`** (`--staged`, `--worktree`, `--source`): Restores working tree or index files from a tree or commit.
- **`clean`** (`-f`, `-d`, `-x`, `-X`): Cleans untracked files and directories with `.gitignore` pattern support.

#### History & Diffing
- **`commit -m <msg>`**: Writes tree and commit objects from the staging index.
- **`log`** (`--oneline`): Walks commit parent graph and displays history.
- **`diff`** (`--staged`, `[<commit> [<commit>]]`): Computes line-by-line Myers diffs between working tree, index, or commits.

#### Branching & Navigation
- **`branch`** (`-d`, `-D`, `-m`): Creates, lists, renames, and safely deletes branches.
- **`switch`** (`-c`, `--detach`, `-f`): Switches branches or checks out detached HEADs with safety checks.
- **`checkout`** (`-b`, `--detach`, `-f`): Classic branch and commit switching interface.

#### Merging, Replaying & Reverting
- **`merge <branch>`**: Performs 3-way tree merges with conflict marker generation (`<<<<<<<`, `=======`, `>>>>>>>`).
- **`cherry-pick <commit>`** (`--no-commit`, `--continue`, `--abort`): Replays a single commit onto current HEAD with state recording.
- **`revert <commit>`** (`--no-commit`, `--continue`, `--abort`): Inverts and applies the changes of a target commit.

#### Reset & Stash Operations
- **`reset`** (`--soft`, `--mixed`, `--hard`, `[-- <paths...>]`): Resets HEAD, index, and working tree.
- **`stash`** (`push`, `pop`, `apply`, `list`, `drop`, `show`, `clear`): Stashes working directory and index state onto `.git/STASH_LIST`.

#### Automated Debugging
- **`bisect`** (`start`, `bad`, `good`, `skip`, `reset`, `log`, `run`): Binary search tool to locate the commit that introduced a bug.

### Plumbing Commands (Low Level)
- **`hash-object`** (`-w`): Computes SHA-1 hash for a file and optionally writes it to object store.
- **`cat-file`** (`-p`): Decompresses and displays raw Git object contents.
- **`write-tree`**: Builds tree objects from the index.
- **`ls-tree`** (`--name-only`): Lists tree object contents.
- **`commit-tree`**: Low-level creation of commit objects.

---

## Installation & Build

Requires [Rust and Cargo](https://rustup.rs/).

```bash
git clone https://github.com/SebiSomu/rgit.git
cd rgit
cargo build --release
```

The binary will be generated at `target/release/rgit`.

---

## Test Harness (`rtest`)

To safely test `rgit` commands without touching the primary repository's `.git` folder, `rgit` includes an isolated test runner (`rtest`):

```bash
# Execute rgit commands within test-sandbox/
cargo run --bin rtest -- init
cargo run --bin rtest -- add .
cargo run --bin rtest -- commit -m "Initial sandbox commit"

# Clean sandbox environment
cargo run --bin rtest -- --clean
```

---

## License

MIT License.
