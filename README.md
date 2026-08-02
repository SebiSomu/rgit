# rgit

A lightweight, locally functional implementation of Git written in Rust.

`rgit` was built to explore the internal architecture of version control systems. Version 1.0.0 successfully implements the core Git object model (blobs, trees, commits), the staging index, and the local repository lifecycle.

## Features (v1.0.0)

* **`init`**: Initializes a new repository with the standard `.git` directory structure.
* **`add`**: Hashes file contents into blobs and stages them in the index.
* **`status`**: Compares the working directory, index, and HEAD to report untracked, modified, and staged files.
* **`commit`**: Generates tree objects from the index and records a new commit in the repository history.

## Installation

Ensure you have [Rust and Cargo](https://rustup.rs/) installed. Clone the repository and build the project:

```bash
git clone <your-repo-url>
cd rgit
cargo build --release
```

The compiled binary will be available in `target/release/rgit`.

## Usage

To prevent conflicts with real Git repositories, it is recommended to test `rgit` in a dedicated, un-versioned subfolder.

```bash
# Create a test directory
mkdir test-repo && cd test-repo

# Initialize an rgit repository
../target/release/rgit init

# Create a file and check status
echo "Hello World" > hello.txt
../target/release/rgit status

# Stage and commit
../target/release/rgit add .
../target/release/rgit commit -m "Initial commit"
```

## Roadmap (Future Features)
* Branching (`branch`, `checkout`)
* Viewing changes (`diff`)
* Restoring files (`restore`)
* `.gitignore` parsing

## License
This project is open-source and available under the MIT License.