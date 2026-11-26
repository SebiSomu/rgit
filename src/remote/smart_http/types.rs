/// One ref a remote advertises, in the shape every transport reports it (protocol v2 `ls-refs`
/// lines, v0 ref advertisements, local repositories, bundles). `oid` is `None` for an unborn
/// ref (a new repository's HEAD); `symref_target` is where a symbolic ref points; `peeled` is
/// the commit an annotated tag points at.
// Used by: src/remote/smart_http.rs, src/remote/ls_remote.rs, src/remote/ls_remote/engine.rs
// Used by command/helper: `discover_refs`, `local_refs`, `bundle_refs` and the `ls-remote` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRef {
    pub name: String,
    pub oid: Option<String>,
    pub symref_target: Option<String>,
    pub peeled: Option<String>,
}

/// Which protocol a smart-HTTP server answered with: 2 when it understood the
/// `Git-Protocol: version=2` header, 0 for older servers (the v0 ref advertisement).
// Used by: src/remote/smart_http.rs
// Used by command/helper: `discover_refs` (and fetch over HTTPS from step 5 on)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolVersion {
    V0,
    V2,
}

/// What ref discovery found: the refs, the protocol, the server's capabilities and the base
/// URL to send later requests to (after any redirect of `info/refs`).
// Used by: src/remote/smart_http.rs, src/remote/ls_remote.rs
// Used by command/helper: `discover_refs` and the `ls-remote` command
#[derive(Debug, Clone)]
pub struct Discovery {
    pub refs: Vec<RemoteRef>,
    pub version: ProtocolVersion,
    pub capabilities: Vec<String>,
    pub base_url: String,
}

/// Picks the `have` lines of a fetch negotiation, as git's default negotiator does: local
/// commits newest first (by committer date), starting from the local ref tips. A commit the
/// server acknowledges is common, and so is everything it reaches: those are never sent.
/// The walk ends when every commit left in the queue is known to be common. `lookup` gives a
/// commit's parents and committer date (`None` for a missing or non-commit object).
// Used by: src/remote/smart_http.rs, src/remote/smart_http/negotiation.rs
// Used by command/helper: `fetch_pack` (fetch, clone and pull over HTTPS)
pub struct Negotiator<F: FnMut(&str) -> Option<(Vec<String>, i64)>> {
    pub(crate) lookup: F,
    /// Commits still to visit, newest first (ties by name, so the order is stable).
    pub(crate) queue: std::collections::BinaryHeap<(i64, String)>,
    /// Commits that were ever queued.
    pub(crate) seen: std::collections::HashSet<String>,
    /// Commits in the queue right now.
    pub(crate) queued: std::collections::HashSet<String>,
    /// Commits known to be common with the server.
    pub(crate) common: std::collections::HashSet<String>,
    /// Parents of every seen commit.
    pub(crate) parents: std::collections::HashMap<String, Vec<String>>,
    /// Queued commits not known to be common; the walk ends when it reaches 0.
    pub(crate) non_common: usize,
}

/// What the server answered to one round of a fetch: the pack (after `done`, or after the
/// server said it is `ready` in protocol v2), or the acknowledged `have`s of a round that
/// did not end the negotiation, with `ready` when the server (v0) has enough to send a pack.
// Used by: src/remote/smart_http.rs
// Used by command/helper: `fetch_pack`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundAnswer {
    Pack,
    Acks { acks: Vec<String>, ready: bool },
}

/// Shows the server's sideband progress text as `remote: ...` lines. The text arrives in
/// chunks that can end in the middle of a line, so the unfinished part waits for the rest.
// Used by: src/remote/smart_http.rs
// Used by command/helper: `fetch_pack`
#[derive(Debug, Default)]
pub struct RemoteProgress {
    /// The start of a line whose end (`\n` or `\r`) has not arrived yet.
    pub(crate) pending: Vec<u8>,
}
