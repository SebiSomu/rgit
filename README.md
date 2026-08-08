# rgit

A lightweight, locally functional implementation of Git written in Rust.

`rgit` was built to explore the internal architecture of version control systems. The core Git object model (blobs, trees, commits), the staging index, the local repository lifecycle, and a full branching system are all implemented.

## Features

### Core Repository (v1.0.0)

- **`init`**: Initializes a new repository with the standard `.git` directory structure.
- **`add`**: Hashes file contents into blobs and stages them in the index.
- **`status`**: Compares the working directory, index, and HEAD to report untracked, modified, and staged files.
- **`commit -m <message>`**: Generates tree objects from the index and records a new commit in the repository history.
- **`log`** / **`log --oneline`**: Walks the commit history from HEAD and prints commit metadata.

### Branching (v1.1.0)

#### `branch` — Branch management

#### `switch` — Modern branch switching

#### `checkout` — Classic Git-style branch operations

(all with their own classic git variations)

## Installation

Ensure you have [Rust and Cargo](https://rustup.rs/) installed. Clone the repository and build the project:

```bash
git clone <your-repo-url>
cd rgit
cargo build --release
```

The compiled binary will be available in `target/release/rgit`.

## Testing & Usage

To prevent conflicts with real Git repositories, this project includes a built-in test harness (`rtest`) that compiles and runs the application inside an isolated `test-sandbox/` directory (which is git-ignored).

### Running Commands in the Sandbox

Prefix your `rgit` commands with `cargo run --bin rtest --`:

```bash
# Initialize a repository inside the sandbox
cargo run --bin rtest -- init

# Stage and commit
cargo run --bin rtest -- add hello.txt
cargo run --bin rtest -- commit -m "Initial commit"

# Work with branches
cargo run --bin rtest -- branch feature-branch
cargo run --bin rtest -- switch feature-branch
cargo run --bin rtest -- switch -c new-feature
cargo run --bin rtest -- checkout -b hotfix main
```

### Cleaning the Sandbox

To wipe the test sandbox completely and start fresh, pass the `--clean` option:

```bash
cargo run --bin rtest -- --clean
```

You can combine it to clean and reinitialize in one command:

```bash
cargo run --bin rtest -- --clean init
```

## Roadmap (Future Features)

- Viewing changes (`diff`)
- Restoring files (`restore`)
- `.gitignore` parsing
- Remote operations (`fetch`, `pull`, `push`)

## License

This project is open-source and available under the MIT License.
