# rgit

A lightweight, locally functional implementation of Git written in Rust.

`rgit` was built to explore the internal architecture of version control systems. Version 1.0.0 successfully implements the core Git object model (blobs, trees, commits), the staging index, and the local repository lifecycle.

## Features (v1.0.0)

- **`init`**: Initializes a new repository with the standard `.git` directory structure.
- **`add`**: Hashes file contents into blobs and stages them in the index.
- **`status`**: Compares the working directory, index, and HEAD to report untracked, modified, and staged files.
- **`commit`**: Generates tree objects from the index and records a new commit in the repository history.

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

# Create a file inside the sandbox and check status
# (Manually create one first, or let rtest run)
cargo run --bin rtest -- status

# Stage and commit in the sandbox
cargo run --bin rtest -- add hello.txt
cargo run --bin rtest -- commit -m "Initial commit"

# List or create branches in the sandbox
cargo run --bin rtest -- branch
cargo run --bin rtest -- branch feature-branch
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

- Branching (`branch`, `checkout`)
- Viewing changes (`diff`)
- Restoring files (`restore`)
- `.gitignore` parsing

## License

This project is open-source and available under the MIT License.
