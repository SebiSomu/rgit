# rgit tests

Everything here pins down what rgit does, so a change that should not alter behaviour can prove it, and a change that should alter it shows exactly what moved.

```
cargo test                  # unit tests (in the rgit binary) + snapshot scenarios
cargo test --test cli merge # only scenarios whose name contains "merge"
```

Today: 95 unit tests, 137 scenarios (140 `.snap` files in `tests/cli/scenarios/snapshots/`) and 18 HTTP tests against real git.

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

## `tests/cli/`: HTTP tests against real git

`http_server.rs` is a small HTTP/1.1 server on 127.0.0.1 that runs `git http-backend` (git's own smart-HTTP program) for every request, serving bare repositories from a temporary directory: the HTTPS transport is tested against a real git server with no network and no accounts. Options: HTTP Basic authentication, a redirect, and protocol v0 only; every request is logged (method, path, headers, status) so tests can check what rgit sent. `plain_path` gives paths Git for Windows accepts (no `\\?\` prefix).

- `smart_http_fetch.rs`: clone and fetch over protocol v2 and v0, with a login, an empty repository, negotiation past many local commits, unrelated histories.
- `smart_http_push.rs`: fast-forwards, new branches, annotated tags, deletions, rejected and forced updates, a refusing server hook, a login.
- `smart_http_tags.rs`: tag auto-follow, `--tags`, `--no-tags`, moved tags.
- `ls_remote_parity.rs`: `rgit ls-remote` output byte for byte against `git ls-remote`.
- `http_server_tests.rs`: the server itself, with real git as the client.

Each result is checked with `git fsck --strict` and real git's view of the refs. The tests skip themselves when `git http-backend` isn't available.

## `tests/unit/`: unit tests

Parsing and formatting helpers. Each file is compiled *inside* its module through a `#[cfg(test)] #[path = "..."] mod tests;` line at the bottom of the source file, so the tests keep access to private items while `src/` holds no test code. When a module moves, fix the `#[path]` in that line.

## `clippy-baseline.txt`

The number of warnings `cargo clippy --bin rgit` reports, in total and per lint (218 at the time of writing). New work should not raise any of them.
