//! Unit tests for `src/remote/smart_http.rs`, compiled into that module via `#[path]`. The
//! network side is covered by the `ls-remote` parity test against real git servers.

use super::*;

#[test]
fn parses_ls_refs_lines() {
    let oid = "1".repeat(40);
    let peeled = "2".repeat(40);
    assert_eq!(
        parse_ls_refs_line(&format!("{} HEAD symref-target:refs/heads/main", oid)).unwrap(),
        RemoteRef { name: "HEAD".into(), oid: Some(oid.clone()), symref_target: Some("refs/heads/main".into()), peeled: None }
    );
    assert_eq!(parse_ls_refs_line(&format!("{} refs/tags/v1 peeled:{}", oid, peeled)).unwrap().peeled, Some(peeled));
    let unborn = parse_ls_refs_line("unborn HEAD symref-target:refs/heads/main").unwrap();
    assert_eq!(unborn.oid, None);
    assert_eq!(unborn.symref_target.as_deref(), Some("refs/heads/main"));
    assert!(parse_ls_refs_line("garbage").is_err());
}

#[test]
fn strips_the_service_header_before_v2_like_github_sends_it() {
    // GitHub's answer to a v2 request starts with the v0-style service header.
    let mut packets = vec![
        Packet::Data(b"# service=git-upload-pack\n".to_vec()),
        Packet::Flush,
        Packet::Data(b"version 2\n".to_vec()),
        Packet::Data(b"agent=git/github-x\n".to_vec()),
        Packet::Data(b"ls-refs=unborn\n".to_vec()),
        Packet::Flush,
    ];
    strip_service_header(&mut packets);
    assert_eq!(packets[0].text().as_deref(), Some("version 2"));
    // git's http-backend sends no header for v2: nothing to strip.
    let mut plain = vec![Packet::Data(b"version 2\n".to_vec()), Packet::Flush];
    strip_service_header(&mut plain);
    assert_eq!(plain.len(), 2);
}

#[test]
fn parses_a_real_v0_advertisement() {
    // The same capture the pkt-line tests use: `git upload-pack --advertise-refs` output.
    let mut packets = crate::remote::pktline::decode_all(include_bytes!("pktline_advertisement.bin")).unwrap();
    strip_service_header(&mut packets);
    let (refs, capabilities) = parse_v0_advertisement(&packets).unwrap();
    assert_eq!(refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["HEAD", "refs/heads/main"]);
    assert_eq!(refs[0].symref_target.as_deref(), Some("refs/heads/main"));
    assert_eq!(refs[0].oid, refs[1].oid);
    assert!(capabilities.iter().any(|c| c == "side-band-64k"));
}

#[test]
fn v0_peeled_tags_and_empty_repositories() {
    let (a, b) = ("a".repeat(40), "b".repeat(40));
    let packets = vec![
        Packet::Data(format!("{} refs/tags/v1\0multi_ack\n", a).into_bytes()),
        Packet::Data(format!("{} refs/tags/v1^{{}}\n", b).into_bytes()),
        Packet::Flush,
    ];
    let (refs, _) = parse_v0_advertisement(&packets).unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].peeled.as_deref(), Some(b.as_str()));
    let empty = vec![Packet::Data(format!("{} capabilities^{{}}\0agent=git/2\n", ZERO_OID).into_bytes()), Packet::Flush];
    assert!(parse_v0_advertisement(&empty).unwrap().0.is_empty());
    let refused = vec![Packet::Data(b"ERR repository disabled\n".to_vec())];
    assert_eq!(parse_v0_advertisement(&refused).unwrap_err().to_string(), "remote error: repository disabled");
}
