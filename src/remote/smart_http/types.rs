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
// `version`, `capabilities` and `base_url` are read by fetch over HTTPS from step 5 on.
#[allow(dead_code)]
pub struct Discovery {
    pub refs: Vec<RemoteRef>,
    pub version: ProtocolVersion,
    pub capabilities: Vec<String>,
    pub base_url: String,
}
