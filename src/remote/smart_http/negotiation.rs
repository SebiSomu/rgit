//! The `have` side of a fetch negotiation (`Negotiator`) and the progress display
//! (`RemoteProgress`) used by `fetch_pack`.

use crate::remote::smart_http::types::{Negotiator, RemoteProgress};

impl<F: FnMut(&str) -> Option<(Vec<String>, i64)>> Negotiator<F> {
    /// Starts a walk from `tips` (local ref tips; objects that are not commits are skipped).
    // Used by `fetch_pack`.
    pub fn new(lookup: F, tips: &[String]) -> Negotiator<F> {
        let mut negotiator = Negotiator {
            lookup,
            queue: Default::default(),
            seen: Default::default(),
            queued: Default::default(),
            common: Default::default(),
            parents: Default::default(),
            non_common: 0,
        };
        for tip in tips {
            negotiator.push(tip);
        }
        negotiator
    }

    /// The next commit to send as `have`, or `None` when there is nothing left worth sending.
    // Used by `fetch_pack`.
    pub fn next_have(&mut self) -> Option<String> {
        while self.non_common > 0 {
            let (_, oid) = self.queue.pop()?;
            self.queued.remove(&oid);
            let is_common = self.common.contains(&oid);
            if !is_common {
                self.non_common -= 1;
            }
            for parent in self.parents.get(&oid).cloned().unwrap_or_default() {
                if is_common {
                    self.make_common(&parent);
                }
                self.push(&parent);
            }
            if !is_common {
                return Some(oid);
            }
        }
        None
    }

    /// Records that the server has `oid` (it acknowledged it): it and its ancestors are common.
    // Used by `fetch_pack`.
    pub fn ack(&mut self, oid: &str) {
        self.make_common(oid);
        for parent in self.parents.get(oid).cloned().unwrap_or_default() {
            self.make_common(&parent);
        }
    }

    /// Marks `oid` common; its ancestors follow as the walk reaches them.
    // Used by `next_have` and `ack`.
    fn make_common(&mut self, oid: &str) {
        if self.common.insert(oid.to_string()) && self.queued.contains(oid) {
            self.non_common -= 1;
        }
    }

    /// Queues `oid` the first time it is seen, if it is a local commit.
    // Used by `new` and `next_have`.
    fn push(&mut self, oid: &str) {
        if !self.seen.insert(oid.to_string()) {
            return;
        }
        let Some((parents, date)) = (self.lookup)(oid) else { return };
        self.parents.insert(oid.to_string(), parents);
        self.queue.push((date, oid.to_string()));
        self.queued.insert(oid.to_string());
        if !self.common.contains(oid) {
            self.non_common += 1;
        }
    }
}

impl RemoteProgress {
    /// Turns a chunk of progress text into finished `remote: ` lines (each ended by `\n` or
    /// `\r`, as the server sent it) and keeps any unfinished line for the next chunk.
    // Used by `fetch_pack` and `RemoteProgress::show`.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for &byte in bytes {
            if byte == b'\n' || byte == b'\r' {
                out.extend_from_slice(b"remote: ");
                out.append(&mut self.pending);
                out.push(byte);
            } else {
                self.pending.push(byte);
            }
        }
        out
    }

    /// The unfinished line left at the end of the transfer, as a last `remote: ` line.
    // Used by `fetch_pack` and `RemoteProgress::finish`.
    pub fn take_rest(&mut self) -> Vec<u8> {
        if self.pending.is_empty() {
            return Vec::new();
        }
        let mut out = b"remote: ".to_vec();
        out.append(&mut self.pending);
        out.push(b'\n');
        out
    }

    /// Feeds a chunk and writes the finished lines to stderr.
    // Used by `fetch_pack`.
    pub fn show(&mut self, bytes: &[u8]) {
        let lines = self.feed(bytes);
        write_stderr(&lines);
    }

    /// Writes the unfinished line, if any, to stderr.
    // Used by `fetch_pack`.
    pub fn finish(&mut self) {
        let rest = self.take_rest();
        write_stderr(&rest);
    }
}

/// Writes bytes to stderr and flushes, ignoring errors (progress is best effort).
// Used by `RemoteProgress::show` and `RemoteProgress::finish`.
fn write_stderr(bytes: &[u8]) {
    use std::io::Write;
    if bytes.is_empty() {
        return;
    }
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(bytes);
    let _ = stderr.flush();
}

#[cfg(test)]
#[path = "../../../tests/unit/remote/smart_http_negotiation.rs"]
mod tests;
