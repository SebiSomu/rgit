/// Says whether git forbids a byte anywhere in a ref name: ASCII control characters, DEL,
/// space, and the characters revision and refspec syntax give a meaning to.
// Used by `check_refname_format`.
fn is_forbidden_ref_byte(b: u8) -> bool {
    b < 0x20 || b == 0x7f || matches!(b, b' ' | b'~' | b'^' | b':' | b'?' | b'[' | b'\\')
}

/// Checks a full ref name against git's rules (`git check-ref-format`): no empty components
/// (so no leading, trailing or doubled `/`), no component starting with `.` or ending in
/// `.lock`, no `..`, no `@{`, no forbidden bytes, not `@`, not ending in `.`, and at least two
/// components unless `allow_onelevel`. With `refspec_pattern`, one `*` is allowed in the
/// whole name. Non-ASCII bytes are allowed, as in git.
// Used by the `check-ref-format` command.
pub fn check_refname_format(name: &str, allow_onelevel: bool, refspec_pattern: bool) -> bool {
    if name == "@" || name.ends_with('.') {
        return false;
    }

    let mut star_allowed = refspec_pattern;
    let mut components = 0;
    for component in name.split('/') {
        if component.is_empty() || component.starts_with('.') || component.ends_with(".lock") {
            return false;
        }
        let mut last = 0u8;
        for &b in component.as_bytes() {
            if is_forbidden_ref_byte(b) || (b == b'.' && last == b'.') || (b == b'{' && last == b'@') {
                return false;
            }
            if b == b'*' {
                if !star_allowed {
                    return false;
                }
                star_allowed = false;
            }
            last = b;
        }
        components += 1;
    }

    components >= 2 || allow_onelevel
}

/// Strips leading slashes and collapses runs of slashes into one, the way git's `--normalize`
/// does. A trailing slash is kept, so `refs/heads/` still fails the format check afterwards.
// Used by the `check-ref-format` command (`--normalize`).
pub fn collapse_ref_slashes(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev = '/';
    for c in name.chars() {
        if c == '/' && prev == '/' {
            continue;
        }
        out.push(c);
        prev = c;
    }
    out
}

/// Checks a branch shorthand the way git's `--branch` does: it must not start with `-`, must
/// not be `HEAD`, and `refs/heads/<name>` must be a valid ref name.
// Used by the `check-ref-format` command (`--branch`).
pub fn is_valid_branch_shorthand(name: &str) -> bool {
    !name.starts_with('-') && name != "HEAD" && check_refname_format(&format!("refs/heads/{}", name), false, false)
}
