//! Characterization tests: snapshots of what the current rgit binary does, so refactors can
//! prove they changed nothing. Update intentionally with `INSTA_UPDATE=always cargo test`
//! (or `cargo insta review`) only when a behaviour change is wanted.

mod harness;
mod http_server;
mod scenarios;
