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
> - **Always** use `cargo run --bin rtest -- [args]` to run test commands.
> - **Never** run `cargo run -- init` or other repository-lifecycle commands directly in the repository root (`c:\rgit-main`), as this will overwrite the main project's actual `.git` configuration and head.

---

## 2. Project Architecture & Components

`rgit` is a lightweight Git implementation in Rust. Its internal architecture maps closely to standard Git specifications:

### Core Modules:
1. **[cli.rs](file:///c:/rgit-main/src/cli.rs)**: Parses and routes all command-line arguments using `clap`. All subcommands (e.g., `Branch`, `Switch`, `Checkout`) are declared here.
2. **[commands.rs](file:///c:/rgit-main/src/commands.rs)**: Core business logic for commands (`init`, `add`, `commit`, `log`, `status`, `branch`, `switch`, `checkout`).
3. **[refs.rs](file:///c:/rgit-main/src/refs.rs)**: Manages reference resolution (`HEAD`, branch heads under `refs/heads/`). Handles attached/detached HEAD state transitions.
4. **[helpers.rs](file:///c:/rgit-main/src/helpers.rs)**: Lower-level helpers for reading and writing compressed Git objects, walking the commit graph (`is_reachable`), and matching file status.
5. **[index.rs](file:///c:/rgit-main/src/index.rs)**: Reads and writes the staging index (`.git/index`).

---

## 3. Best Practices for Implementation

- **Error Handling**: Use `anyhow::Result` and add context with `.context()` or `.with_context()` to help debug file system issues.
- **Branch Validation**: All branch creation and renaming must pass through `refs::is_valid_branch_name` to prevent formatting conflicts (following `git-check-ref-format`).
- **Graph Walks**: Use the BFS-based `is_reachable` ancestor walk in `helpers.rs` rather than assuming linear history (handles branched or merged histories).
- **Nested Directory Handling**: When deleting branch refs, clean up parent directories under `.git/refs/heads/` recursively if they become empty (so nested namespaces like `feature/login` don't leave empty folder structures).
- **Index Sorting**: The `.git/index` file must have entries sorted alphabetically by path. Use `BTreeMap` or sort vectors before serialization.
