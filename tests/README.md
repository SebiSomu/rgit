# rgit tests

Everything here pins down what rgit does, so a change that should not alter behaviour can prove it, and a change that should alter it shows exactly what moved.

```
cargo test                  # unit tests (in the rgit binary) + snapshot scenarios
cargo test --test cli merge # only scenarios whose name contains "merge"
```

Today: 31 unit tests and 118 scenarios (one `.snap` file each in `tests/cli/scenarios/snapshots/`).

## `tests/cli/`: end-to-end scenarios

Each test in `scenarios/` drives the real `rgit` binary through a small story in a fresh temp directory and snapshots (with [insta](https://insta.rs)) one transcript containing every command's stdout, stderr and non-zero exit code, plus a final dump of HEAD, refs, packed-refs, state files in `.git`, the index (`ls-files -s`), the HEAD commit object and the worktree.

`harness.rs` makes the transcript reproducible:

- isolated `HOME` (`XDG_CONFIG_HOME` is removed too, so your global ignore file cannot leak in), fixed author/committer identity, `GIT_*_DATE` starting at 1700000000 and advancing one minute per command; `GIT_*`, `RGIT_*`, `RUST_*` (backtraces!), `EDITOR`, `PAGER` from your shell are removed;
- object ids are relabelled `<H1>`, `<H2>`, ... by first appearance (abbreviations become `<H1:7>`), so code paths that stamp the current time (rebase, cherry-pick, stash, reflog) stay comparable; unknown 40-hex ids become `<Xn>`;
- current-time timestamps become `<NOW>` / `<NOW-DATE>`, relative ages ("3 days ago", from `whereami`, `doctor` and others) become `<AGE> ago`, temp paths `<REPO>`, `<HOME>`, `<TMP>`.

Helpers on `Repo`: `run`, `run_in`, `run_stdin`, `write`, `read`, `remove`, `path`, `set_env`, `unset_env`, `age(path, secs)` (backdates a file's mtime, for stale-lock checks) and `finish` (builds the transcript).

**A failing snapshot means behaviour changed.** If the change is a bug, fix the code, not the snapshot. Only when it is intended: `INSTA_UPDATE=always cargo test --test cli` (or `cargo install cargo-insta` + `cargo insta review`), then review the `.snap` diff in git.

Adding a scenario: write a `#[test]` in `scenarios/<name>.rs` using `Repo::init()` (or `Repo::new()` for commands that run outside a repository), declare it with `mod <name>;` in `scenarios/mod.rs`, end with `assert_snapshot!(r.finish())`, run once with `INSTA_UPDATE=always`, read the new `.snap` and commit it. Do not put real secrets in a test: build fake keys at run time so they never appear in a file or a snapshot.

Scenarios run on Linux and Windows. Snapshots in this repository use CRLF line endings; keep them as they are.

## `tests/unit/`: unit tests

Parsing and formatting helpers. Each file is compiled *inside* its module through a `#[cfg(test)] #[path = "..."] mod tests;` line at the bottom of the source file, so the tests keep access to private items while `src/` holds no test code. When a module moves, fix the `#[path]` in that line.

## `clippy-baseline.txt`

The number of warnings `cargo clippy --bin rgit` reports (226 at the time of writing). New work should not raise it.
