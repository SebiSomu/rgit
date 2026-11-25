//! The secret detection shared by `rgit secrets` and the `add` / `commit` warnings: content
//! rules (AWS, GitHub, Slack, Google, Stripe, Anthropic/OpenAI-style keys, private keys, JWTs,
//! credentials in URLs, hard-coded passwords) and file names that usually hold secrets.

use std::sync::OnceLock;

use regex::Regex;

use crate::workspace::secrets::types::Finding;

/// A line containing this is never reported (for test fixtures and documented examples).
pub const ALLOW_MARKER: &str = "rgit:allow-secret";

/// The content rules, each with what it finds (with its article, as the warnings say it).
// Used by `scan_text` and `ui::narrate::scan_risks`.
pub fn rules() -> &'static [(Regex, &'static str)] {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RULES.get_or_init(|| {
        [
            (r"AKIA[0-9A-Z]{16}", "an AWS access key"),
            (r#"(?i)aws_?secret_?access_?key\s*[:=]\s*['"]?[A-Za-z0-9/+=]{40}"#, "an AWS secret key"),
            (r"-----BEGIN [A-Z ]*PRIVATE KEY( BLOCK)?-----", "a private key"),
            (r"gh[pousr]_[A-Za-z0-9]{36,}", "a GitHub token"),
            (r"github_pat_[A-Za-z0-9_]{60,}", "a GitHub token"),
            (r"xox[baprs]-[A-Za-z0-9-]{10,}", "a Slack token"),
            (r"AIza[0-9A-Za-z_\-]{35}", "a Google API key"),
            (r"(?:sk|rk)_live_[0-9a-zA-Z]{24,}", "a Stripe key"),
            (r"sk-ant-[A-Za-z0-9_\-]{20,}", "an Anthropic API key"),
            (r"sk-(?:proj-)?[A-Za-z0-9]{32,}", "an OpenAI-style API key"),
            (r"eyJ[A-Za-z0-9_\-]{10,}\.eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}", "a JSON web token"),
            (r"[a-zA-Z][a-zA-Z0-9+.\-]*://[^/\s:@]+:[^/\s:@]{3,}@[^\s/]+", "a password in a URL"),
            (r#"(?i)(api[_-]?key|secret|passwd|password|token)\s*[:=]\s*['"][^'"\s]{8,}['"]"#, "a hard-coded password or key"),
        ]
        .iter()
        .filter_map(|(re, what)| Regex::new(re).ok().map(|r| (r, *what)))
        .collect()
    })
}

/// What a file name says the file holds, when it is a usual home of secrets (`.env`, private
/// keys, keystores, cloud credentials); `.env.example` and the like don't count.
// Used by `scan_name` and `ui::narrate::scan_risks`.
pub fn is_secret_file(path: &str) -> bool {
    let lower = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let example = ["example", "sample", "template", "dist"].iter().any(|s| lower.ends_with(&format!(".{}", s)));
    (lower == ".env" || lower.starts_with(".env.") && !example)
        || ["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa", "credentials.json", ".npmrc", ".pypirc", ".netrc"].contains(&lower.as_str())
        || [".pem", ".p12", ".pfx", ".key", ".keystore", ".jks"].iter().any(|e| lower.ends_with(e))
}

/// `AKIA…MP` : the first four and last two characters (all stars when short).
// Used by `scan_text`.
fn mask(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len());
    }
    format!("{}…{}", chars[..4].iter().collect::<String>(), chars[chars.len() - 2..].iter().collect::<String>())
}

/// The findings in `lines` (each with its 1-based line number) of `path`; at most one per line
/// (the first rule that matches), lines with `ALLOW_MARKER` skipped.
// Used by the `secrets` command.
pub fn scan_text<'a>(path: &str, lines: impl Iterator<Item = (usize, &'a str)>, commit: Option<&str>) -> Vec<Finding> {
    let mut out = Vec::new();
    for (n, line) in lines {
        if line.contains(ALLOW_MARKER) {
            continue;
        }
        if let Some((m, what)) = rules().iter().find_map(|(re, what)| re.find(line).map(|m| (m, *what))) {
            out.push(Finding { commit: commit.map(str::to_string), path: path.to_string(), line: Some(n), what: what.to_string(), masked: mask(m.as_str()) });
        }
    }
    out
}

/// A finding for a file whose name says it holds secrets.
// Used by the `secrets` command.
pub fn scan_name(path: &str, commit: Option<&str>) -> Option<Finding> {
    is_secret_file(path).then(|| Finding { commit: commit.map(str::to_string), path: path.to_string(), line: None, what: "a secrets/key file".to_string(), masked: String::new() })
}

/// A blob's text when it is text and at most 1 MiB (secrets in bigger or binary files aren't
/// looked for).
// Used by the `secrets` command.
pub fn blob_text(hash: &[u8; 20]) -> anyhow::Result<Option<String>> {
    let content = crate::core::object_store::read_object(&hex::encode(hash))?.1;
    if content.len() > 1 << 20 || content.contains(&0) {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&content).to_string()))
}

/// The lines `new` adds to `old` (1-based numbers in `new`): every line when there is no old.
// Used by the `secrets` command for staged changes and commits.
pub fn added_lines(old: Option<&str>, new: &str) -> Vec<(usize, String)> {
    let new_lines: Vec<&str> = new.lines().collect();
    let Some(old) = old else {
        return new_lines.iter().enumerate().map(|(i, l)| (i + 1, l.to_string())).collect();
    };
    let old_lines: Vec<&str> = old.lines().collect();
    let mut out = Vec::new();
    let mut pos = 0;
    for op in crate::diff::myers::myers_diff(&old_lines, &new_lines) {
        match op {
            crate::diff::types::DiffOp::Keep(_) => pos += 1,
            crate::diff::types::DiffOp::Insert(line) => {
                pos += 1;
                out.push((pos, line.to_string()));
            }
            crate::diff::types::DiffOp::Delete(_) => {}
        }
    }
    out
}
