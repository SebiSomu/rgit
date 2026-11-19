# rgit tests

Safety net for the refactor: everything here pins down what rgit does **today**, so a
change that should be behaviour-neutral can prove it is.

```
cargo test                  # unit tests (in the rgit binary) + snapshot scenarios
cargo test --test cli merge # only scenarios whose name contains "merge"
```

## `tests/cli/` — characterization scenarios

Each test in `scenarios/` drives the real `rgit` binary through a small story in a fresh
temp directory and snapshots (with [insta](https://insta.rs)) one transcript containing
every command's stdout, stderr and non-zero exit code, plus a final dump of HEAD, refs,
packed-refs, state files in `.git`, the index (`ls-files -s`), the HEAD commit object and the
worktree.

`harness.rs` makes the transcript reproducible:

- isolated `HOME`, fixed author/committer identity, `GIT_*_DATE` starting at 1700000000 and
  advancing one minute per command; `GIT_*`, `RGIT_*`, `RUST_*` (backtraces!), `EDITOR`,
  `PAGER` from your shell are removed;
- object ids are relabelled `<H1>`, `<H2>`, ... by first appearance (abbreviations become
  `<H1:7>`), so code paths that stamp the current time (rebase, cherry-pick, stash, reflog)
  stay comparable; unknown 40-hex ids become `<Xn>`;
- current-time timestamps become `<NOW>` / `<NOW-DATE>`, temp paths `<REPO>`, `<HOME>`, `<TMP>`.

**A failing snapshot during the refactor means behaviour changed — fix the code, not the
snapshot.** Only when a change is intended: `INSTA_UPDATE=always cargo test --test cli`
(or `cargo install cargo-insta` + `cargo insta review`), then review the `.snap` diff in git.

Adding a scenario: write a `#[test]` using `Repo::init()` / the fixtures in
`scenarios/mod.rs`, end with `assert_snapshot!(r.finish())`, run once with
`INSTA_UPDATE=always`, read the new `.snap` and commit it.

## `tests/unit/` — unit tests

The unit tests removed from `src/` in `024e755`, restored unchanged. Each file is compiled
*inside* its module through a `#[cfg(test)] #[path = "..."] mod tests;` line at the bottom of
the source file, so they keep access to private items while `src/` holds no test code.
When a module moves during the refactor, fix the `#[path]` in that line.

## `clippy-baseline.txt`

Warning counts before the refactor; the refactor should not raise any of them.
