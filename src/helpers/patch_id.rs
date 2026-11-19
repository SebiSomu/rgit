use sha1::{Digest, Sha1};
use crate::core::config::get_config_value;
use crate::objects::PatchIdOptions;

/// git's usage text for `patch-id`, printed for `-h` and after option errors (exit 129).
pub const PATCH_ID_USAGE: &str = "usage: rgit patch-id [--stable | --unstable | --verbatim]

    --unstable            use the unstable patch-id algorithm
    --stable              use the stable patch-id algorithm
    --verbatim            don't strip whitespace from the patch
";

/// Parses `patch-id` arguments like git: `--unstable`, `--stable` and `--verbatim` are
/// mutually exclusive modes; without one, `patchid.stable` / `patchid.verbatim` from the
/// config decide (verbatim implies stable). Errors carry the usage text and mean exit 129.
// Used by `patch-id`.
pub fn parse_patch_id_args(args: &[String]) -> anyhow::Result<PatchIdOptions> {
    let mut mode: Option<&str> = None;
    for arg in args {
        match arg.as_str() {
            "--unstable" | "--stable" | "--verbatim" => {
                if let Some(previous) = mode.filter(|m| *m != arg.as_str()) {
                    anyhow::bail!("error: options '{}' and '{}' cannot be used together\n{}", arg, previous, PATCH_ID_USAGE);
                }
                mode = Some(arg.as_str());
            }
            "--" => break,
            other if other.starts_with('-') && other.len() > 1 => {
                let name = other.trim_start_matches('-');
                let kind = if other.starts_with("--") { "option" } else { "switch" };
                anyhow::bail!("error: unknown {} `{}'\n{}", kind, name, PATCH_ID_USAGE);
            }
            _ => {}
        }
    }

    let config_bool = |key: &str| get_config_value(key).ok().flatten().is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"));
    Ok(match mode {
        Some("--unstable") => PatchIdOptions { stable: false, verbatim: false },
        Some("--stable") => PatchIdOptions { stable: true, verbatim: false },
        Some(_) => PatchIdOptions { stable: true, verbatim: true },
        None => {
            let verbatim = config_bool("patchid.verbatim");
            PatchIdOptions { stable: verbatim || config_bool("patchid.stable"), verbatim }
        }
    })
}

/// Reads a stream of patches (`diff`, `log -p` or `format-patch` output) and returns one
/// `(patch id, commit id)` pair per patch, exactly like git's `generate_id_list`: a line
/// that starts with a 40-hex object id (optionally after `commit ` or `From `) begins the
/// next patch and names its commit (all zeros when there is none); patches without any
/// diff lines produce no pair.
// Used by `patch-id`.
pub fn compute_patch_ids(input: &[u8], opts: &PatchIdOptions) -> Vec<(String, String)> {
    let lines: Vec<&[u8]> = input.split_inclusive(|&b| b == b'\n').collect();
    let mut out = Vec::new();
    let mut pos = 0;
    let mut commit_id = "0".repeat(40);
    loop {
        let (patch_len, result, next_id) = one_patch_id(&lines, &mut pos, opts);
        if patch_len > 0 {
            out.push((hex::encode(result), commit_id.clone()));
        }
        commit_id = next_id.unwrap_or_else(|| "0".repeat(40));
        if pos >= lines.len() {
            break;
        }
    }
    out
}

/// Port of git's `get_one_patchid`: hashes one patch starting at `lines[*pos]`, skipping
/// commit messages, `index` lines and hunk line numbers, and returns the hashed length,
/// the patch id, and the object id that ended the patch (the next commit), if any.
// Used by `patch-id` (internal to this file's helpers).
fn one_patch_id(lines: &[&[u8]], pos: &mut usize, opts: &PatchIdOptions) -> (usize, [u8; 20], Option<String>) {
    let mut patch_len = 0usize;
    let (mut before, mut after) = (-1i64, -1i64);
    let mut diff_is_binary = false;
    let (mut pre_oid, mut post_oid): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
    let mut hasher = Sha1::new();
    let mut result = [0u8; 20];
    let mut next_id = None;

    while *pos < lines.len() {
        let line = lines[*pos];
        *pos += 1;

        let prefixed = line.strip_prefix(b"commit ").or_else(|| line.strip_prefix(b"From "));
        if prefixed.is_none() && line.starts_with(b"\\ ") && line.len() > 12 {
            if opts.verbatim {
                hasher.update(line);
            }
            continue;
        }
        let candidate = prefixed.unwrap_or(line);
        if candidate.len() >= 40 && candidate[..40].iter().all(|b| b.is_ascii_hexdigit()) {
            next_id = Some(String::from_utf8_lossy(&candidate[..40]).to_ascii_lowercase());
            break;
        }

        // Commit message and other text before the first `diff` line.
        if patch_len == 0 && !line.starts_with(b"diff ") {
            continue;
        }

        // Inside a diff header?
        if before == -1 {
            if line.starts_with(b"GIT binary patch") || line.starts_with(b"Binary files") {
                diff_is_binary = true;
                before = 0;
                hasher.update(&pre_oid);
                hasher.update(&post_oid);
                if opts.stable {
                    flush_one_hunk(&mut result, &mut hasher);
                }
                continue;
            } else if let Some(rest) = line.strip_prefix(b"index ") {
                if let Some(dots) = find_bytes(rest, b"..") {
                    let tail = &rest[dots + 2..];
                    let end = tail.iter().position(|&b| b == b' ').unwrap_or(tail.len().saturating_sub(1));
                    pre_oid = rest[..dots].to_vec();
                    post_oid = tail[..end].to_vec();
                }
                continue;
            } else if line.starts_with(b"--- ") {
                before = 1;
                after = 1;
            } else if !line.first().is_some_and(|b| b.is_ascii_alphabetic()) {
                break;
            }
        }

        if diff_is_binary {
            if line.starts_with(b"diff ") {
                diff_is_binary = false;
                before = -1;
            }
            continue;
        }

        // Between hunks: a hunk header, another file's header, or the end of the patch.
        if before == 0 && after == 0 {
            if line.starts_with(b"@@ -") {
                scan_hunk_header(line, &mut before, &mut after);
                continue;
            }
            if !line.starts_with(b"diff ") {
                break;
            }
            if opts.stable {
                flush_one_hunk(&mut result, &mut hasher);
            }
            before = -1;
            after = -1;
        }

        // Inside a hunk (or a header line that is hashed as is).
        if line[0] == b'-' || line[0] == b' ' {
            before -= 1;
        }
        if line[0] == b'+' || line[0] == b' ' {
            after -= 1;
        }

        if opts.verbatim {
            patch_len += line.len();
            hasher.update(line);
        } else {
            let stripped: Vec<u8> = line.iter().copied().filter(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r')).collect();
            patch_len += stripped.len();
            hasher.update(&stripped);
        }
    }

    flush_one_hunk(&mut result, &mut hasher);
    (patch_len, result, next_id)
}

/// Port of git's `scan_hunk_header`: reads the old and new line counts of `@@ -a,b +c,d @@`
/// (a missing count means 1) so the hunk body can be followed without its line numbers.
// Used by `patch-id` (internal to this file's helpers).
fn scan_hunk_header(line: &[u8], before: &mut i64, after: &mut i64) -> bool {
    let digits = |s: &[u8]| s.iter().take_while(|b| b.is_ascii_digit()).count();
    let number = |s: &[u8]| -> i64 { String::from_utf8_lossy(&s[..digits(s)]).parse().unwrap_or(0) };
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);

    let mut q = &line[4..];
    let mut n = digits(q);
    if at(q, n) == b',' {
        q = &q[n + 1..];
        *before = number(q);
        n = digits(q);
    } else {
        *before = 1;
    }
    if n == 0 || at(q, n) != b' ' || at(q, n + 1) != b'+' {
        return false;
    }

    let mut r = &q[n + 2..];
    n = digits(r);
    if at(r, n) == b',' {
        r = &r[n + 1..];
        *after = number(r);
        n = digits(r);
    } else {
        *after = 1;
    }
    n != 0
}

/// Port of git's `flush_one_hunk`: finishes the running SHA-1, adds it bytewise (with
/// carry) into `result`, and starts a fresh hash.
// Used by `patch-id` (internal to this file's helpers).
fn flush_one_hunk(result: &mut [u8; 20], hasher: &mut Sha1) {
    let hash: [u8; 20] = std::mem::take(hasher).finalize().into();
    let mut carry: u16 = 0;
    for (byte, add) in result.iter_mut().zip(hash) {
        carry += *byte as u16 + add as u16;
        *byte = carry as u8;
        carry >>= 8;
    }
}

/// Position of the first occurrence of `needle` in `haystack`.
// Used by `patch-id` (internal to this file's helpers).
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
