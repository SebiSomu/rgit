/// One pkt-line as read from a git server: a payload, or one of the special packets that
/// carry no payload. `Flush` (`0000`) ends a section or message, `Delim` (`0001`) separates
/// parts of a protocol v2 request or response, `ResponseEnd` (`0002`) ends a v2 response.
// Used by: src/remote/pktline.rs
// Used by command/helper: `PacketReader`, `demux_sideband` (and from step 4 on, `ls-remote`, `fetch` and `push` over HTTPS)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    Data(Vec<u8>),
    Flush,
    Delim,
    ResponseEnd,
}

impl Packet {
    /// The payload as a line of text: one trailing newline removed, as git does. `None` for
    /// the special packets.
    pub fn text(&self) -> Option<String> {
        match self {
            Packet::Data(bytes) => {
                let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
                Some(String::from_utf8_lossy(bytes).into_owned())
            }
            _ => None,
        }
    }

    /// The message of an `ERR <message>` packet, which a server sends instead of a normal
    /// response when it refuses a request.
    pub fn error_message(&self) -> Option<String> {
        self.text().and_then(|line| line.strip_prefix("ERR ").map(str::to_string))
    }
}

/// One sideband packet, split by its first byte: band 1 carries pack data, band 2 progress
/// text for the user (`remote: Counting objects...`), band 3 a fatal error from the server.
// Used by: src/remote/pktline.rs
// Used by command/helper: `parse_sideband` and `demux_sideband`
#[derive(Debug, Clone, PartialEq, Eq)]
// Used by fetch over HTTPS from step 5 on.
#[allow(dead_code)]
pub enum Band {
    Data(Vec<u8>),
    Progress(Vec<u8>),
    Error(Vec<u8>),
}
