pub mod backup;
pub mod bundle;
pub mod clone;
pub mod credential;
pub mod fetch;
pub mod github;
pub mod http;
pub mod open;
pub mod pktline;
pub mod pr_summary;
pub mod pull;
pub mod push;
// The `remote` command is named after its domain (`remote/remote.rs`), by convention.
#[allow(clippy::module_inception)]
pub mod remote;
pub mod sync;
pub mod transport;
pub mod types;
