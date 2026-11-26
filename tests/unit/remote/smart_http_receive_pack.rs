//! Unit tests for `src/remote/smart_http/receive_pack.rs`, compiled into that module via
//! `#[path]`. The exchange with a real server is covered by `tests/cli/smart_http_push.rs`.

use super::*;

fn caps(list: &[&str]) -> Vec<String> {
    list.iter().map(|c| c.to_string()).collect()
}

#[test]
fn the_first_command_carries_the_capabilities_then_the_pack_follows() {
    let update = RefUpdate { name: "refs/heads/main".into(), old: "1".repeat(40), new: "2".repeat(40) };
    let offered = caps(&["report-status", "delete-refs", "side-band-64k", "quiet", "ofs-delta", "agent=git/2.4"]);
    let body = push_request(&offered, std::slice::from_ref(&update), b"PACK...", false).unwrap();
    let text = String::from_utf8_lossy(&body);
    let command = format!("{} {} refs/heads/main\0report-status side-band-64k quiet ofs-delta agent=", "1".repeat(40), "2".repeat(40));
    assert!(text[4..].starts_with(&command), "{:?}", text);
    assert!(body.ends_with(b"0000PACK..."), "{:?}", text);
    // On a terminal the server's progress is wanted.
    let body = push_request(&offered, std::slice::from_ref(&update), b"", true).unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("quiet"));
}

#[test]
fn a_deletion_sends_no_pack() {
    let update = RefUpdate { name: "refs/heads/old".into(), old: "1".repeat(40), new: ZERO_OID.into() };
    let body = push_request(&caps(&["report-status", "delete-refs"]), &[update], b"PACK...", false).unwrap();
    assert!(body.ends_with(b"0000"), "{:?}", String::from_utf8_lossy(&body));
}

#[test]
fn parses_the_report() {
    let lines = |texts: &[&str]| {
        let mut packets: Vec<Packet> = texts.iter().map(|t| Packet::Data(format!("{}\n", t).into_bytes())).collect();
        packets.push(Packet::Flush);
        packets
    };
    let report = parse_report_status(&lines(&["unpack ok", "ok refs/heads/main", "ng refs/heads/x pre-receive hook declined"])).unwrap();
    assert_eq!(report.unpack_error, None);
    assert_eq!(report.refs, vec![("refs/heads/main".into(), None), ("refs/heads/x".into(), Some("pre-receive hook declined".into()))]);
    let report = parse_report_status(&lines(&["unpack index-pack failed", "ng refs/heads/main unpacker error"])).unwrap();
    assert_eq!(report.unpack_error.as_deref(), Some("index-pack failed"));
    assert!(parse_report_status(&[Packet::Flush]).is_err());
    assert!(parse_report_status(&lines(&["unpack ok", "what"])).is_err());
}
