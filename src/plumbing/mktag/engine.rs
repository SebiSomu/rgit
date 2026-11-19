use crate::core::check_ref_format::check_refname_format;

/// Checks one `tagger` value like git's `fsck_ident`: `Name <email> <time> <+HHMM>`.
// Used by `check_tag_text`.
fn check_ident(line: &[u8]) -> Option<(&'static str, &'static str)> {
    let at = |i: usize| line.get(i).copied().unwrap_or(b'\n');
    let mut p = 0;
    if at(p) == b'<' {
        return Some(("missingNameBeforeEmail", "invalid author/committer line - missing space before email"));
    }
    while !matches!(at(p), b'<' | b'>' | b'\n') {
        p += 1;
    }
    if at(p) == b'>' {
        return Some(("badName", "invalid author/committer line - bad name"));
    }
    if at(p) != b'<' {
        return Some(("missingEmail", "invalid author/committer line - missing email"));
    }
    if p == 0 || line[p - 1] != b' ' {
        return Some(("missingSpaceBeforeEmail", "invalid author/committer line - missing space before email"));
    }
    p += 1;
    while !matches!(at(p), b'<' | b'>' | b'\n') {
        p += 1;
    }
    if at(p) != b'>' {
        return Some(("badEmail", "invalid author/committer line - bad email"));
    }
    p += 1;
    if at(p) != b' ' {
        return Some(("missingSpaceBeforeDate", "invalid author/committer line - missing space before date"));
    }
    p += 1;
    while matches!(at(p), b' ' | b'\t') {
        p += 1;
    }
    if !at(p).is_ascii_digit() {
        return Some(("badDate", "invalid author/committer line - bad date"));
    }
    if at(p) == b'0' && at(p + 1) != b' ' {
        return Some(("zeroPaddedDate", "invalid author/committer line - zero-padded date"));
    }
    let start = p;
    while at(p).is_ascii_digit() {
        p += 1;
    }
    if String::from_utf8_lossy(&line[start..p]).parse::<i64>().is_err() {
        return Some(("badDateOverflow", "invalid author/committer line - date causes integer overflow"));
    }
    if at(p) != b' ' {
        return Some(("badDate", "invalid author/committer line - bad date"));
    }
    p += 1;
    let zone_ok = matches!(at(p), b'+' | b'-') && (1..=4).all(|i| at(p + i).is_ascii_digit()) && at(p + 5) == b'\n';
    if !zone_ok {
        return Some(("badTimezone", "invalid author/committer line - bad time zone"));
    }
    None
}

/// Checks tag object text like git's `fsck_tag_standalone`: the header must be terminated,
/// and hold `object <hash>`, `type <type>`, `tag <name>` and `tagger <ident>` lines in that
/// order with nothing after them. Returns the tagged object's hash and type as far as they
/// could be read, and the problems as `(fsck message id, description, warning only without
/// strict)`: the first one found (later checks are skipped, as in git, except
/// after problems that are only warnings without `strict`).
// Used by `mktag`.
pub fn check_tag_text(text: &[u8], strict: bool) -> (Option<String>, Option<String>, Vec<(&'static str, String, bool)>) {
    let mut problems: Vec<(&'static str, String, bool)> = Vec::new();
    let fatal = |problems: &mut Vec<(&'static str, String, bool)>, id: &'static str, message: String| problems.push((id, message, false));

    // verify_headers: no NUL in the header, and the header must end with a newline.
    let mut terminated = false;
    for (i, b) in text.iter().enumerate() {
        if *b == 0 {
            fatal(&mut problems, "nulInHeader", format!("unterminated header: NUL at offset {}", i));
            return (None, None, problems);
        }
        if *b == b'\n' && text.get(i + 1) == Some(&b'\n') {
            terminated = true;
            break;
        }
    }
    if !terminated && text.last() != Some(&b'\n') {
        fatal(&mut problems, "unterminatedHeader", "unterminated header".to_string());
        return (None, None, problems);
    }

    let line_end = |from: usize| text[from..].iter().position(|b| *b == b'\n').map(|i| from + i);
    let Some(rest) = text.strip_prefix(b"object ") else {
        fatal(&mut problems, "missingObject", "invalid format - expected 'object' line".to_string());
        return (None, None, problems);
    };
    if rest.len() < 41 || rest[40] != b'\n' || !rest[..40].iter().all(|b| b.is_ascii_hexdigit()) {
        fatal(&mut problems, "badObjectSha1", "invalid 'object' line format - bad sha1".to_string());
        return (None, None, problems);
    }
    let object = Some(String::from_utf8_lossy(&rest[..40]).to_lowercase());
    let mut pos = 7 + 41;

    if !text[pos..].starts_with(b"type ") {
        fatal(&mut problems, "missingTypeEntry", "invalid format - expected 'type' line".to_string());
        return (object, None, problems);
    }
    pos += 5;
    let Some(end) = line_end(pos) else {
        fatal(&mut problems, "missingType", "invalid format - unexpected end after 'type' line".to_string());
        return (object, None, problems);
    };
    let tagged_type = String::from_utf8_lossy(&text[pos..end]).to_string();
    if !matches!(tagged_type.as_str(), "blob" | "tree" | "commit" | "tag") {
        fatal(&mut problems, "badType", "invalid 'type' value".to_string());
        return (object, None, problems);
    }
    pos = end + 1;

    if !text[pos..].starts_with(b"tag ") {
        fatal(&mut problems, "missingTagEntry", "invalid format - expected 'tag' line".to_string());
        return (object, Some(tagged_type), problems);
    }
    pos += 4;
    let Some(end) = line_end(pos) else {
        fatal(&mut problems, "missingTag", "invalid format - unexpected end after 'type' line".to_string());
        return (object, Some(tagged_type), problems);
    };
    let name = String::from_utf8_lossy(&text[pos..end]).to_string();
    if !check_refname_format(&format!("refs/tags/{}", name), false, false) {
        problems.push(("badTagName", format!("invalid 'tag' name: {}", name), true));
        if strict {
            return (object, Some(tagged_type), problems);
        }
    }
    pos = end + 1;

    if !text[pos..].starts_with(b"tagger ") {
        problems.push(("missingTaggerEntry", "invalid format - expected 'tagger' line".to_string(), true));
        if strict {
            return (object, Some(tagged_type), problems);
        }
    } else {
        pos += 7;
        let ident_problem = check_ident(&text[pos..]);
        pos = line_end(pos).map(|e| e + 1).unwrap_or(text.len());
        if let Some((id, message)) = ident_problem {
            fatal(&mut problems, id, message.to_string());
            return (object, Some(tagged_type), problems);
        }
    }
    if pos != text.len() && text[pos] != b'\n' {
        problems.push(("extraHeaderEntry", "invalid format - extra header(s) after 'tagger'".to_string(), true));
    }
    (object, Some(tagged_type), problems)
}
