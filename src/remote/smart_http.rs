//! git's smart-HTTP protocol (gitprotocol-http, gitprotocol-v2) on top of `HttpClient` and
//! pkt-line: ref discovery through `info/refs`, then protocol v2 `ls-refs` when the server
//! speaks v2, or the v0 ref advertisement when it doesn't. Fetch and push build on this.

pub mod types;

use anyhow::Result;

use crate::remote::http::{redact_url, HttpClient};
use crate::remote::pktline::types::Packet;
use crate::remote::pktline::{decode_all, demux_sideband, encode_text, PacketReader, DELIM, FLUSH};
use crate::remote::smart_http::types::{Discovery, ProtocolVersion, RemoteRef};

const ADVERTISEMENT_TYPE: &str = "application/x-git-upload-pack-advertisement";
const REQUEST_TYPE: &str = "application/x-git-upload-pack-request";
const RESULT_TYPE: &str = "application/x-git-upload-pack-result";
const ZERO_OID: &str = "0000000000000000000000000000000000000000";

/// Lists the refs of the repository at `url` (`https://host/owner/repo.git`). Asks for protocol
/// v2 and falls back to v0 when the server answers with it. `prefixes` (`refs/heads/`,
/// `refs/tags/`) are passed to v2's `ls-refs` so the server can send less; v0 always sends
/// everything, so callers filter again. `unborn` also asks v2 for an unborn HEAD (clone wants
/// it; `ls-remote` doesn't, like git).
// Used by `ls-remote` (and fetch over HTTPS from step 5 on).
pub fn discover_refs(client: &HttpClient, url: &str, prefixes: &[String], unborn: bool) -> Result<Discovery> {
    let base = url.trim_end_matches('/').to_string();
    let info_refs = format!("{}/info/refs?service=git-upload-pack", base);
    let response = client.get(&info_refs, &[("Git-Protocol", "version=2")])?.check_status(url)?;
    if response.content_type().as_deref() != Some(ADVERTISEMENT_TYPE) {
        anyhow::bail!(
            "fatal: '{}' is not a git smart-HTTP server (dumb HTTP is not supported)",
            redact_url(url)
        );
    }
    // A redirect of info/refs moves the whole remote, as in git.
    let final_url = response.final_url().to_string();
    let base = final_url.strip_suffix("/info/refs?service=git-upload-pack").map(str::to_string).unwrap_or(base);
    let mut packets = decode_all(&response.bytes()?)?;
    strip_service_header(&mut packets);

    if packets.first().and_then(Packet::text).as_deref() == Some("version 2") {
        let capabilities: Vec<String> = packets
            .iter()
            .skip(1)
            .take_while(|p| matches!(p, Packet::Data(_)))
            .filter_map(Packet::text)
            .collect();
        let refs = ls_refs(client, &base, url, &capabilities, prefixes, unborn)?;
        return Ok(Discovery { refs, version: ProtocolVersion::V2, capabilities, base_url: base });
    }
    let (refs, capabilities) = parse_v0_advertisement(&packets)?;
    Ok(Discovery { refs, version: ProtocolVersion::V0, capabilities, base_url: base })
}

/// Removes the `# service=git-upload-pack` line and its flush that smart servers may send
/// first: before the v0 refs always, before `version 2` on some servers (GitHub) but not
/// others (git's http-backend).
// Used by `discover_refs`.
pub fn strip_service_header(packets: &mut Vec<Packet>) {
    if packets.first().and_then(Packet::text).is_some_and(|t| t.starts_with("# service=")) {
        packets.remove(0);
        if packets.first() == Some(&Packet::Flush) {
            packets.remove(0);
        }
    }
}

/// Parses a v0 ref advertisement (after `strip_service_header`): an optional `version 1`,
/// then `<oid> <name>` lines up to a flush, the first carrying the capabilities after a NUL,
/// `^{}` lines peeling the tag before them, and `symref=` capabilities naming symbolic refs.
// Used by `discover_refs`.
pub fn parse_v0_advertisement(packets: &[Packet]) -> Result<(Vec<RemoteRef>, Vec<String>)> {
    let mut refs: Vec<RemoteRef> = Vec::new();
    let mut capabilities = Vec::new();
    for packet in packets {
        let Some(line) = packet.text() else { break };
        if line == "version 1" {
            continue;
        }
        if let Some(message) = packet.error_message() {
            anyhow::bail!("remote error: {}", message);
        }
        let (refline, caps) = match line.split_once('\0') {
            Some((refline, caps)) => (refline.to_string(), Some(caps.to_string())),
            None => (line.clone(), None),
        };
        if let Some(caps) = caps {
            capabilities = caps.split(' ').filter(|c| !c.is_empty()).map(str::to_string).collect();
        }
        let Some((oid, name)) = refline.split_once(' ') else {
            anyhow::bail!("protocol error: unexpected ref line '{}'", refline);
        };
        if name == "capabilities^{}" && oid == ZERO_OID {
            continue; // an empty repository advertises only its capabilities
        }
        if let Some(tag) = name.strip_suffix("^{}") {
            if let Some(last) = refs.last_mut().filter(|r| r.name == tag) {
                last.peeled = Some(oid.to_string());
            }
            continue;
        }
        refs.push(RemoteRef { name: name.to_string(), oid: Some(oid.to_string()), symref_target: None, peeled: None });
    }
    for capability in &capabilities {
        if let Some((from, to)) = capability.strip_prefix("symref=").and_then(|s| s.split_once(':'))
            && let Some(r) = refs.iter_mut().find(|r| r.name == from)
        {
            r.symref_target = Some(to.to_string());
        }
    }
    Ok((refs, capabilities))
}

/// Protocol v2 `ls-refs`: asks for peeled tags, symref targets and (with `unborn`, when the
/// server offers it) an unborn HEAD, limited to `prefixes`, and parses the answer.
// Used by `discover_refs`.
fn ls_refs(client: &HttpClient, base: &str, url: &str, capabilities: &[String], prefixes: &[String], unborn: bool) -> Result<Vec<RemoteRef>> {
    let ls_refs_caps = capabilities.iter().find_map(|c| if c == "ls-refs" { Some("") } else { c.strip_prefix("ls-refs=") });
    let Some(ls_refs_caps) = ls_refs_caps else {
        anyhow::bail!("fatal: the server at '{}' speaks protocol v2 but does not offer ls-refs", redact_url(url));
    };
    let mut body = encode_text("command=ls-refs")?;
    if capabilities.iter().any(|c| c.starts_with("agent=")) {
        body.extend(encode_text(&format!("agent={}", crate::remote::http::USER_AGENT))?);
    }
    if capabilities.iter().any(|c| c == "object-format=sha1") {
        body.extend(encode_text("object-format=sha1")?);
    }
    body.extend_from_slice(DELIM);
    body.extend(encode_text("peel")?);
    body.extend(encode_text("symrefs")?);
    if unborn && ls_refs_caps.split(' ').any(|c| c == "unborn") {
        body.extend(encode_text("unborn")?);
    }
    for prefix in prefixes {
        body.extend(encode_text(&format!("ref-prefix {}", prefix))?);
    }
    body.extend_from_slice(FLUSH);

    let headers = [("Content-Type", REQUEST_TYPE), ("Accept", RESULT_TYPE), ("Git-Protocol", "version=2")];
    let response = client.post(&format!("{}/git-upload-pack", base), &headers, &body)?.check_status(url)?;
    let mut refs = Vec::new();
    for packet in decode_all(&response.bytes()?)? {
        if let Some(message) = packet.error_message() {
            anyhow::bail!("remote error: {}", message);
        }
        let Some(line) = packet.text() else { break };
        refs.push(parse_ls_refs_line(&line)?);
    }
    Ok(refs)
}

/// One `ls-refs` answer line: `<oid> <name>[ symref-target:<ref>][ peeled:<oid>]`, or
/// `unborn <name> symref-target:<ref>` for a branch with no commits yet.
// Used by `ls_refs`.
pub fn parse_ls_refs_line(line: &str) -> Result<RemoteRef> {
    let mut fields = line.split(' ');
    let (Some(oid), Some(name)) = (fields.next(), fields.next()) else {
        anyhow::bail!("protocol error: unexpected ls-refs line '{}'", line);
    };
    let mut entry = RemoteRef {
        name: name.to_string(),
        oid: (oid != "unborn").then(|| oid.to_string()),
        symref_target: None,
        peeled: None,
    };
    for attribute in fields {
        if let Some(target) = attribute.strip_prefix("symref-target:") {
            entry.symref_target = Some(target.to_string());
        } else if let Some(peeled) = attribute.strip_prefix("peeled:") {
            entry.peeled = Some(peeled.to_string());
        }
    }
    Ok(entry)
}

/// Downloads one pack holding `wants` (object ids, normally branch tips) and everything they
/// reach, from a server found by `discover_refs`. Uses protocol v2's `fetch` command, or the
/// v0 request for older servers. Sends no `have` lines yet, so the pack is complete on its
/// own. With `progress`, the server's progress text is shown as `remote: ...` lines (git shows
/// it only when stderr is a terminal); otherwise the server is asked not to send it.
/// Returns the raw pack bytes, still to be verified and indexed by the caller.
// Used by `fetch` and `clone` over HTTPS.
pub fn fetch_pack(client: &HttpClient, discovery: &Discovery, url: &str, wants: &[String], progress: bool) -> Result<Vec<u8>> {
    let (body, v2) = match discovery.version {
        ProtocolVersion::V2 => (v2_fetch_request(&discovery.capabilities, wants, progress)?, true),
        ProtocolVersion::V0 => (v0_fetch_request(&discovery.capabilities, wants, progress)?, false),
    };
    let mut headers = vec![("Content-Type", REQUEST_TYPE), ("Accept", RESULT_TYPE)];
    if v2 {
        headers.push(("Git-Protocol", "version=2"));
    }
    let response = client.post(&format!("{}/git-upload-pack", discovery.base_url), &headers, &body)?.check_status(url)?;
    let mut reader = PacketReader::new(response.into_reader());
    let mut pack = Vec::new();
    let mut show = |bytes: &[u8]| {
        if progress {
            show_remote_progress(bytes);
        }
    };
    if v2 {
        read_v2_fetch_response(&mut reader, &mut pack, &mut show)?;
    } else {
        read_v0_fetch_response(&mut reader, &mut pack, &mut show, discovery.capabilities.iter().any(|c| c == "side-band-64k" || c == "side-band"))?;
    }
    if pack.is_empty() {
        anyhow::bail!("fatal: the server at '{}' sent no pack", redact_url(url));
    }
    Ok(pack)
}

/// The v2 `fetch` request: the command, then `ofs-delta`, one `want` per object, `done`.
// Used by `fetch_pack`.
fn v2_fetch_request(capabilities: &[String], wants: &[String], progress: bool) -> Result<Vec<u8>> {
    if !capabilities.iter().any(|c| c == "fetch" || c.starts_with("fetch=")) {
        anyhow::bail!("fatal: the server speaks protocol v2 but does not offer fetch");
    }
    let mut body = encode_text("command=fetch")?;
    if capabilities.iter().any(|c| c.starts_with("agent=")) {
        body.extend(encode_text(&format!("agent={}", crate::remote::http::USER_AGENT))?);
    }
    if capabilities.iter().any(|c| c == "object-format=sha1") {
        body.extend(encode_text("object-format=sha1")?);
    }
    body.extend_from_slice(DELIM);
    body.extend(encode_text("ofs-delta")?);
    if !progress {
        body.extend(encode_text("no-progress")?);
    }
    for want in wants {
        body.extend(encode_text(&format!("want {}", want))?);
    }
    body.extend(encode_text("done")?);
    body.extend_from_slice(FLUSH);
    Ok(body)
}

/// The v0 request: the first `want` carries the capabilities rgit uses (of those the server
/// offered), then the other `want`s, a flush and `done`.
// Used by `fetch_pack`.
fn v0_fetch_request(capabilities: &[String], wants: &[String], progress: bool) -> Result<Vec<u8>> {
    let offered = |name: &str| capabilities.iter().any(|c| c == name);
    let mut requested: Vec<String> = Vec::new();
    if offered("side-band-64k") {
        requested.push("side-band-64k".into());
    } else if offered("side-band") {
        requested.push("side-band".into());
    }
    if offered("ofs-delta") {
        requested.push("ofs-delta".into());
    }
    if !progress && offered("no-progress") {
        requested.push("no-progress".into());
    }
    if capabilities.iter().any(|c| c.starts_with("agent=")) {
        requested.push(format!("agent={}", crate::remote::http::USER_AGENT));
    }
    let mut body = Vec::new();
    for (i, want) in wants.iter().enumerate() {
        let line = if i == 0 { format!("want {} {}", want, requested.join(" ")) } else { format!("want {}", want) };
        body.extend(encode_text(line.trim_end())?);
    }
    body.extend_from_slice(FLUSH);
    body.extend(encode_text("done")?);
    Ok(body)
}

/// Reads a v2 `fetch` response: sections (`shallow-info`, `wanted-refs`, `packfile-uris`) are
/// skipped up to the `packfile` section, whose sideband stream is the pack.
// Used by `fetch_pack`.
fn read_v2_fetch_response<R: std::io::Read>(reader: &mut PacketReader<R>, pack: &mut Vec<u8>, progress: &mut dyn FnMut(&[u8])) -> Result<()> {
    while let Some(packet) = reader.read_packet()? {
        if let Some(message) = packet.error_message() {
            anyhow::bail!("remote error: {}", message);
        }
        match packet.text().as_deref() {
            Some("packfile") => {
                demux_sideband(reader, pack, progress)?;
                return Ok(());
            }
            // A section header or one of its lines; sections end with a delimiter.
            Some(_) | None => continue,
        }
    }
    anyhow::bail!("protocol error: the server's answer ended before the pack")
}

/// Reads a v0 response to a request ending in `done`: `NAK` (no common commits), then the
/// pack, in sideband packets when side-band was negotiated, raw otherwise.
// Used by `fetch_pack`.
fn read_v0_fetch_response<R: std::io::Read>(reader: &mut PacketReader<R>, pack: &mut Vec<u8>, progress: &mut dyn FnMut(&[u8]), sideband: bool) -> Result<()> {
    loop {
        let Some(packet) = reader.read_packet()? else {
            anyhow::bail!("protocol error: the server's answer ended before the pack");
        };
        if let Some(message) = packet.error_message() {
            anyhow::bail!("remote error: {}", message);
        }
        match packet.text().as_deref() {
            Some("NAK") => break,
            Some(line) if line.starts_with("ACK ") => continue,
            Some(line) if line.starts_with("shallow ") || line.starts_with("unshallow ") => continue,
            _ => anyhow::bail!("protocol error: expected NAK from the server"),
        }
    }
    if sideband {
        demux_sideband(reader, pack, progress)?;
    } else {
        reader.read_raw_to_end(pack)?;
    }
    Ok(())
}

/// Shows sideband progress text as git does: each line (ended by `\n` or `\r`) prefixed
/// with `remote: `.
// Used by `fetch_pack`.
fn show_remote_progress(bytes: &[u8]) {
    use std::io::Write;
    let text = String::from_utf8_lossy(bytes);
    let mut stderr = std::io::stderr().lock();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c == '\n' || c == '\r' {
            let _ = write!(stderr, "remote: {}{}", &text[start..i], c);
            start = i + 1;
        }
    }
    if start < text.len() {
        let _ = write!(stderr, "remote: {}", &text[start..]);
    }
    let _ = stderr.flush();
}

#[cfg(test)]
#[path = "../../tests/unit/remote/smart_http.rs"]
mod tests;
