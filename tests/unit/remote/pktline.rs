//! Unit tests for `src/remote/pktline.rs`, compiled into that module via `#[path]`.

use super::*;

#[test]
fn encodes_lengths_like_git() {
    assert_eq!(encode(b"a\n").unwrap(), b"0006a\n");
    assert_eq!(encode_text("command=ls-refs").unwrap(), b"0014command=ls-refs\n");
    assert_eq!(encode_text("done\n").unwrap(), b"0009done\n");
    assert!(encode(b"").is_err());
    assert_eq!(encode(&vec![b'x'; MAX_PAYLOAD_LEN]).unwrap()[..4], *b"fff0");
    assert!(encode(&vec![b'x'; MAX_PAYLOAD_LEN + 1]).is_err());
}

#[test]
fn reads_data_and_special_packets() {
    let stream = b"0006a\n0000000100020009done\n";
    assert_eq!(
        decode_all(stream).unwrap(),
        vec![Packet::Data(b"a\n".to_vec()), Packet::Flush, Packet::Delim, Packet::ResponseEnd, Packet::Data(b"done\n".to_vec())]
    );
    // Upper-case hex is accepted when reading.
    assert_eq!(decode_all(b"000Ahello\n").unwrap(), vec![Packet::Data(b"hello\n".to_vec())]);
    assert_eq!(decode_all(b"").unwrap(), vec![]);
}

#[test]
fn rejects_broken_streams() {
    assert!(decode_all(b"00").is_err(), "stream ends inside a length");
    assert!(decode_all(b"0009do").is_err(), "stream ends inside a payload");
    assert!(decode_all(b"0003").is_err(), "0003 is never valid");
    assert!(decode_all(b"zzzz").is_err(), "length isn't hex");
    assert!(decode_all(b"fff1").is_err(), "over the size limit");
}

#[test]
fn text_strips_exactly_one_newline() {
    assert_eq!(Packet::Data(b"a b\n".to_vec()).text().as_deref(), Some("a b"));
    assert_eq!(Packet::Data(b"x\n\n".to_vec()).text().as_deref(), Some("x\n"));
    assert_eq!(Packet::Data(b"no newline".to_vec()).text().as_deref(), Some("no newline"));
    assert_eq!(Packet::Flush.text(), None);
}

#[test]
fn err_packets_stop_reading() {
    assert_eq!(Packet::Data(b"ERR access denied\n".to_vec()).error_message().as_deref(), Some("access denied"));
    let mut reader = PacketReader::new(&b"0006a\n0016ERR access denied\n"[..]);
    assert_eq!(reader.read_until_flush().unwrap_err().to_string(), "remote error: access denied");
}

#[test]
fn reads_up_to_each_flush() {
    let mut reader = PacketReader::new(&b"0006a\n0006b\n00000006c\n0000"[..]);
    assert_eq!(reader.read_until_flush().unwrap(), vec![b"a\n".to_vec(), b"b\n".to_vec()]);
    assert_eq!(reader.read_until_flush().unwrap(), vec![b"c\n".to_vec()]);
    assert_eq!(reader.read_until_flush().unwrap(), Vec::<Vec<u8>>::new());
}

/// Builds one sideband packet.
fn band(number: u8, bytes: &[u8]) -> Vec<u8> {
    let mut payload = vec![number];
    payload.extend_from_slice(bytes);
    encode(&payload).unwrap()
}

#[test]
fn sideband_splits_data_progress_and_errors() {
    let mut stream = Vec::new();
    stream.extend(band(2, b"Counting objects: 3\r"));
    stream.extend(band(1, b"PACK"));
    stream.extend(band(1, b"\x00\x00\x00\x02"));
    stream.extend(band(2, b"done.\n"));
    stream.extend_from_slice(FLUSH);
    let mut data = Vec::new();
    let mut progress = Vec::new();
    let written = demux_sideband(&mut PacketReader::new(&stream[..]), &mut data, &mut |b: &[u8]| progress.extend_from_slice(b)).unwrap();
    assert_eq!(written, 8);
    assert_eq!(data, b"PACK\x00\x00\x00\x02");
    assert_eq!(progress, b"Counting objects: 3\rdone.\n");

    let mut stream = band(1, b"PA");
    stream.extend(band(3, b"upload-pack: not our ref\n"));
    let err = demux_sideband(&mut PacketReader::new(&stream[..]), &mut Vec::new(), &mut |_: &[u8]| {}).unwrap_err();
    assert_eq!(err.to_string(), "remote error: upload-pack: not our ref");

    assert!(parse_sideband(&[7, b'x']).is_err(), "unknown band");
    assert!(parse_sideband(&[]).is_err(), "empty sideband packet");
}

#[test]
fn decodes_a_real_git_ref_advertisement() {
    // Captured from `git upload-pack --advertise-refs` on a one-commit repository.
    let advertisement = include_bytes!("pktline_advertisement.bin");
    let mut reader = PacketReader::new(&advertisement[..]);
    let lines: Vec<String> = reader.read_until_flush().unwrap().into_iter().map(|p| Packet::Data(p).text().unwrap()).collect();
    assert!(lines[0].contains(" HEAD\0"), "first line carries the capabilities after a NUL: {:?}", lines[0]);
    assert!(lines.iter().any(|l| l.ends_with(" refs/heads/main")));
    assert_eq!(reader.read_packet().unwrap(), None, "the advertisement ends with a flush");
}
