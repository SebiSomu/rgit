//! The push side of git's smart-HTTP protocol: `git-receive-pack` ref discovery, the request
//! (ref update commands, then the pack) and the server's `report-status` answer. Receive-pack
//! has no protocol v2, so this is always the v0 exchange, as in git.

use anyhow::Result;

use crate::remote::http::{redact_url, HttpClient};
use crate::remote::pktline::types::Packet;
use crate::remote::pktline::{decode_all, demux_sideband, encode_text, PacketReader, FLUSH};
use crate::remote::smart_http::types::{Discovery, ProtocolVersion, PushReport, RefUpdate, RemoteProgress};
use crate::remote::smart_http::{parse_v0_advertisement, strip_service_header};

const ADVERTISEMENT_TYPE: &str = "application/x-git-receive-pack-advertisement";
const REQUEST_TYPE: &str = "application/x-git-receive-pack-request";
const RESULT_TYPE: &str = "application/x-git-receive-pack-result";

/// Lists the refs of the repository at `url` as the push service sees them, with its
/// capabilities (`report-status`, `delete-refs`, `side-band-64k`, `atomic`, ...). Servers
/// usually want a login already here (GitHub does, even for a public repository), which the
/// client's login loop asks the credential helpers for.
// Used by `push` over HTTPS.
pub fn discover_receive_pack(client: &HttpClient, url: &str) -> Result<Discovery> {
    let base = url.trim_end_matches('/').to_string();
    let info_refs = format!("{}/info/refs?service=git-receive-pack", base);
    let response = client.get(&info_refs, &[])?.check_status(url)?;
    if response.content_type().as_deref() != Some(ADVERTISEMENT_TYPE) {
        anyhow::bail!("fatal: '{}' does not accept pushes over smart HTTP", redact_url(url));
    }
    // A redirect of info/refs moves the whole remote, as in git.
    let final_url = response.final_url().to_string();
    let base = final_url.strip_suffix("/info/refs?service=git-receive-pack").map(str::to_string).unwrap_or(base);
    let mut packets = decode_all(&response.bytes()?)?;
    strip_service_header(&mut packets);
    let (refs, capabilities) = parse_v0_advertisement(&packets)?;
    Ok(Discovery { refs, version: ProtocolVersion::V0, capabilities, base_url: base })
}

/// Sends the ref `updates` and, when any of them creates or moves a ref, `pack` (the objects
/// the server lacks; it may hold no objects at all) to the server found by
/// `discover_receive_pack`, and returns its report. Server messages (band 2: progress,
/// hook output, GitHub's "Create a pull request" hint) are shown as `remote: ...` lines; with
/// `progress` off the server is asked to be `quiet`.
// Used by `push` over HTTPS.
pub fn send_pack(client: &HttpClient, discovery: &Discovery, url: &str, updates: &[RefUpdate], pack: &[u8], progress: bool) -> Result<PushReport> {
    let offered = |name: &str| discovery.capabilities.iter().any(|c| c == name);
    if !offered("report-status") {
        anyhow::bail!("fatal: the server at '{}' does not report push results (no report-status)", redact_url(url));
    }
    let sideband = offered("side-band-64k");
    let body = push_request(&discovery.capabilities, updates, pack, progress)?;
    let headers = [("Content-Type", REQUEST_TYPE), ("Accept", RESULT_TYPE)];
    let response = client.post(&format!("{}/git-receive-pack", discovery.base_url), &headers, &body)?.check_status(url)?;
    let mut reader = PacketReader::new(response.into_reader());
    let report_bytes = if sideband {
        let mut report = Vec::new();
        let mut remote_progress = RemoteProgress::default();
        demux_sideband(&mut reader, &mut report, &mut |bytes| remote_progress.show(bytes))?;
        remote_progress.finish();
        report
    } else {
        let mut raw = Vec::new();
        reader.read_raw_to_end(&mut raw)?;
        raw
    };
    parse_report_status(&decode_all(&report_bytes)?)
}

/// The request body: one `<old> <new> <ref>` line per update, the first carrying the
/// capabilities rgit uses (of those offered), a flush, then the pack unless every update is
/// a deletion (receive-pack expects no pack then).
// Used by `send_pack`.
pub fn push_request(capabilities: &[String], updates: &[RefUpdate], pack: &[u8], progress: bool) -> Result<Vec<u8>> {
    let offered = |name: &str| capabilities.iter().any(|c| c == name);
    let mut requested: Vec<String> = vec!["report-status".into()];
    if offered("side-band-64k") {
        requested.push("side-band-64k".into());
    }
    if !progress && offered("quiet") {
        requested.push("quiet".into());
    }
    if offered("ofs-delta") {
        requested.push("ofs-delta".into());
    }
    if capabilities.iter().any(|c| c.starts_with("agent=")) {
        requested.push(format!("agent={}", crate::remote::http::USER_AGENT));
    }
    let mut body = Vec::new();
    for (i, update) in updates.iter().enumerate() {
        let command = format!("{} {} {}", update.old, update.new, update.name);
        let line = if i == 0 { format!("{}\0{}", command, requested.join(" ")) } else { command };
        body.extend(encode_text(&line)?);
    }
    body.extend_from_slice(FLUSH);
    if updates.iter().any(|u| u.new != ZERO_OID) {
        body.extend_from_slice(pack);
    }
    Ok(body)
}

/// Parses `report-status`: `unpack ok` or `unpack <reason>`, then `ok <ref>` or
/// `ng <ref> <reason>` per ref, up to a flush.
// Used by `send_pack`.
pub fn parse_report_status(packets: &[Packet]) -> Result<PushReport> {
    let mut lines = packets.iter().map_while(Packet::text);
    let Some(unpack) = lines.next() else {
        anyhow::bail!("protocol error: the server sent no push report");
    };
    let Some(unpack) = unpack.strip_prefix("unpack ") else {
        anyhow::bail!("protocol error: unexpected push report line '{}'", unpack);
    };
    let mut report = PushReport { unpack_error: (unpack != "ok").then(|| unpack.to_string()), refs: Vec::new() };
    for line in lines {
        if let Some(name) = line.strip_prefix("ok ") {
            report.refs.push((name.to_string(), None));
        } else if let Some(rest) = line.strip_prefix("ng ") {
            let (name, reason) = rest.split_once(' ').unwrap_or((rest, "failed"));
            report.refs.push((name.to_string(), Some(reason.to_string())));
        } else {
            anyhow::bail!("protocol error: unexpected push report line '{}'", line);
        }
    }
    Ok(report)
}

/// The all-zero object name: the `old` of a ref being created, the `new` of one being deleted.
// Used by `push_request` and `push` over HTTPS.
pub const ZERO_OID: &str = "0000000000000000000000000000000000000000";

#[cfg(test)]
#[path = "../../../tests/unit/remote/smart_http_receive_pack.rs"]
mod tests;
