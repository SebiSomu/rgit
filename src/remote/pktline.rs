//! git's pkt-line framing (gitprotocol-common), shared by every network transport: each
//! packet is a 4-digit hex length (counting those 4 bytes) and a payload, plus three special
//! packets without one (`0000` flush, `0001` delimiter, `0002` response end). Also the
//! sideband split that multiplexes pack data, progress and errors over one packet stream.

pub mod types;

use std::io::{Read, Write};
use anyhow::Result;

use crate::remote::pktline::types::{Band, Packet};

/// The flush packet: ends a section (a ref list, a request, a sideband stream).
pub const FLUSH: &[u8] = b"0000";
/// The delimiter packet: separates the parts of a protocol v2 request or response.
pub const DELIM: &[u8] = b"0001";
/// The response-end packet: ends a protocol v2 response.
// Used by push over HTTPS from step 6 on.
#[allow(dead_code)]
pub const RESPONSE_END: &[u8] = b"0002";

/// The largest packet git sends or accepts, length prefix included (`LARGE_PACKET_MAX`).
pub const MAX_PACKET_LEN: usize = 65520;
/// The largest payload of one packet.
pub const MAX_PAYLOAD_LEN: usize = MAX_PACKET_LEN - 4;

/// Frames `payload` as one packet. Fails for an empty payload (`0004` is legal but never
/// sent, and easy to confuse with a flush) or one longer than `MAX_PAYLOAD_LEN`.
// Used by `encode_text` (and from step 4 on, the HTTPS requests).
pub fn encode(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.is_empty() {
        anyhow::bail!("refusing to write an empty pkt-line");
    }
    if payload.len() > MAX_PAYLOAD_LEN {
        anyhow::bail!("pkt-line payload of {} bytes is over the {} byte limit", payload.len(), MAX_PAYLOAD_LEN);
    }
    let mut packet = format!("{:04x}", payload.len() + 4).into_bytes();
    packet.extend_from_slice(payload);
    Ok(packet)
}

/// Frames one line of text, adding the newline git expects at the end of text packets.
// Used by the HTTPS requests (from step 4 on).
pub fn encode_text(line: &str) -> Result<Vec<u8>> {
    if line.ends_with('\n') {
        encode(line.as_bytes())
    } else {
        encode(format!("{}\n", line).as_bytes())
    }
}

/// Reads packets one at a time from a byte stream (an HTTP response body, an SSH pipe).
// Used by `decode_all`, `demux_sideband` (and from step 4 on, the HTTPS responses).
pub struct PacketReader<R: Read> {
    inner: R,
}

impl<R: Read> PacketReader<R> {
    /// Wraps a byte stream.
    pub fn new(inner: R) -> PacketReader<R> {
        PacketReader { inner }
    }

    /// The next packet, or `None` when the stream ends cleanly between packets. A stream that
    /// ends inside a packet, or a length that isn't 4 hex digits, `0003`, or over the limit,
    /// is an error.
    pub fn read_packet(&mut self) -> Result<Option<Packet>> {
        let mut header = [0u8; 4];
        let mut filled = 0;
        while filled < header.len() {
            let n = self.inner.read(&mut header[filled..])?;
            if n == 0 {
                if filled == 0 {
                    return Ok(None);
                }
                anyhow::bail!("the remote end hung up in the middle of a pkt-line length");
            }
            filled += n;
        }
        let text = std::str::from_utf8(&header).ok().filter(|t| t.bytes().all(|b| b.is_ascii_hexdigit()));
        let Some(len) = text.and_then(|t| usize::from_str_radix(t, 16).ok()) else {
            anyhow::bail!("protocol error: bad line length character: {}", String::from_utf8_lossy(&header));
        };
        match len {
            0 => return Ok(Some(Packet::Flush)),
            1 => return Ok(Some(Packet::Delim)),
            2 => return Ok(Some(Packet::ResponseEnd)),
            3 => anyhow::bail!("protocol error: bad line length {}", len),
            _ if len > MAX_PACKET_LEN => anyhow::bail!("protocol error: bad line length {}", len),
            _ => {}
        }
        let mut payload = vec![0u8; len - 4];
        self.inner
            .read_exact(&mut payload)
            .map_err(|_| anyhow::anyhow!("the remote end hung up in the middle of a pkt-line"))?;
        Ok(Some(Packet::Data(payload)))
    }

    /// Everything left in the stream, unframed (a v0 pack sent without sideband).
    pub fn read_raw_to_end(&mut self, out: &mut Vec<u8>) -> Result<()> {
        self.inner.read_to_end(out)?;
        Ok(())
    }

    /// Reads data packets up to the next flush (or the end of the stream), failing on an
    /// `ERR` packet. Used for v0 ref advertisements and v2 `ls-refs` answers.
    // Used by push over HTTPS from step 6 on (the per-ref report).
    #[allow(dead_code)]
    pub fn read_until_flush(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut payloads = Vec::new();
        while let Some(packet) = self.read_packet()? {
            if let Some(message) = packet.error_message() {
                anyhow::bail!("remote error: {}", message);
            }
            match packet {
                Packet::Data(bytes) => payloads.push(bytes),
                Packet::Flush | Packet::ResponseEnd => break,
                Packet::Delim => {}
            }
        }
        Ok(payloads)
    }
}

/// Decodes a whole buffer of packets (a small HTTP response) at once.
// Used by the HTTPS ref discovery (from step 4 on).
pub fn decode_all(bytes: &[u8]) -> Result<Vec<Packet>> {
    let mut reader = PacketReader::new(bytes);
    let mut packets = Vec::new();
    while let Some(packet) = reader.read_packet()? {
        packets.push(packet);
    }
    Ok(packets)
}

/// Splits a sideband payload by its band byte.
// Used by `demux_sideband`.
pub fn parse_sideband(payload: &[u8]) -> Result<Band> {
    let Some((&band, rest)) = payload.split_first() else {
        anyhow::bail!("protocol error: empty sideband packet");
    };
    match band {
        1 => Ok(Band::Data(rest.to_vec())),
        2 => Ok(Band::Progress(rest.to_vec())),
        3 => Ok(Band::Error(rest.to_vec())),
        other => anyhow::bail!("protocol error: bad band #{}", other),
    }
}

/// Reads a sideband stream up to its flush: pack data goes to `data`, progress text to
/// `progress`, and an error band ends it with `remote error: <message>`. Returns how many
/// data bytes were written.
// Used by fetch and push over HTTPS (from step 5 on).
pub fn demux_sideband<R: Read, W: Write>(reader: &mut PacketReader<R>, data: &mut W, progress: &mut dyn FnMut(&[u8])) -> Result<u64> {
    let mut written = 0u64;
    while let Some(packet) = reader.read_packet()? {
        let payload = match packet {
            Packet::Data(payload) => payload,
            Packet::Flush | Packet::ResponseEnd => break,
            Packet::Delim => continue,
        };
        match parse_sideband(&payload)? {
            Band::Data(bytes) => {
                data.write_all(&bytes)?;
                written += bytes.len() as u64;
            }
            Band::Progress(bytes) => progress(&bytes),
            Band::Error(bytes) => {
                let message = String::from_utf8_lossy(&bytes);
                anyhow::bail!("remote error: {}", message.trim_end());
            }
        }
    }
    Ok(written)
}

#[cfg(test)]
#[path = "../../tests/unit/remote/pktline.rs"]
mod tests;
