//! Atom values for one ref: object fields, people, describe output (ref-filter.c's `grab_values`).

use crate::core::{object_store::{abbreviate_hash_min, read_object}, wildmatch::wildmatch};
use crate::references::{
    for_each_ref::engine::atoms::{header_value, peel_to_commit, peel_to_commit_reporting, tag_target},
    for_each_ref::engine::dates::{parse_ref_date_mode, parse_strtol_prefix, show_ref_date},
    for_each_ref::engine::message::grab_message_value,
    for_each_ref::engine::refs::resolve_ref_chain,
    for_each_ref::engine::upstream::{ahead_behind, branch_push_ref, branch_upstream, remote_ref_details, show_ref_name},
    for_each_ref::types::{RefArrayItem, RefAtomArg, RefAtomKind, RefAtomValue, RefDateMode, RefFilterAtom},
    tag::engine::describe_commit_matching,
};
use std::fs;
use std::collections::HashMap;

/// Computes every atom's value for one ref, like git's `populate_value`: ref-level atoms first
/// (`%(refname)`, `%(upstream)`, `%(HEAD)`, colors, `%(if)`...), then values read from the
/// object the ref names, and with `%(*...)` atoms from the object an annotated tag points at.
/// Atoms that don't apply are empty. Errors are git's fatal messages.
// Used by `for-each-ref`.
pub fn populate_ref_values(item: &RefArrayItem, atoms: &[RefFilterAtom], worktrees: &HashMap<String, String>) -> Result<Vec<RefAtomValue>, String> {
    let mut values: Vec<Option<RefAtomValue>> = vec![None; atoms.len()];
    let text = |s: String| Some(RefAtomValue { s: s.into_bytes(), ..Default::default() });
    let mut reported_non_commit = false;
    let symref_target = if item.symref { resolve_ref_chain(&item.refname).map(|(n, _, _)| n).unwrap_or_default() } else { String::new() };

    for (i, atom) in atoms.iter().enumerate() {
        use RefAtomKind as K;
        let refname = match atom.kind {
            K::Refname => {
                let RefAtomArg::Refname(show) = &atom.arg else { continue };
                show_ref_name(*show, &item.refname)
            }
            K::Symref => {
                let RefAtomArg::Refname(show) = &atom.arg else { continue };
                if item.symref { show_ref_name(*show, &symref_target) } else { String::new() }
            }
            K::WorktreePath => {
                values[i] = text(if item.refname.starts_with("refs/heads/") { worktrees.get(&item.refname).cloned().unwrap_or_default() } else { String::new() });
                continue;
            }
            K::Upstream => {
                let s = match item.refname.strip_prefix("refs/heads/") {
                    Some(branch) => match branch_upstream(branch) {
                        Some(up) => remote_ref_details(&atom.arg, Some(&up), branch),
                        None => String::new(),
                    },
                    None => String::new(),
                };
                values[i] = text(s);
                continue;
            }
            K::Push => {
                let s = match item.refname.strip_prefix("refs/heads/") {
                    Some(branch) => {
                        let push_remote = matches!(atom.arg, RefAtomArg::RemoteRef { push_remote: true, .. });
                        if push_remote {
                            remote_ref_details(&atom.arg, None, branch)
                        } else {
                            match branch_push_ref(branch) {
                                Some(dst) => remote_ref_details(&atom.arg, Some(&dst), branch),
                                None => String::new(),
                            }
                        }
                    }
                    None => String::new(),
                };
                values[i] = text(s);
                continue;
            }
            K::Color => {
                let RefAtomArg::Color(c) = &atom.arg else { continue };
                values[i] = text(c.clone());
                continue;
            }
            K::Flag => {
                let mut flags = Vec::new();
                if item.symref {
                    flags.push("symref");
                }
                if item.packed {
                    flags.push("packed");
                }
                values[i] = text(flags.join(","));
                continue;
            }
            K::ObjectName if !atom.deref => {
                let RefAtomArg::Oid(len) = &atom.arg else { continue };
                values[i] = text(show_oid(&item.oid, *len));
                continue;
            }
            K::Head => {
                let RefAtomArg::Head(head) = &atom.arg else { continue };
                values[i] = text(if head.as_deref() == Some(item.refname.as_str()) { "*" } else { " " }.to_string());
                continue;
            }
            K::Align | K::End | K::Then | K::Else | K::Rest => {
                values[i] = text(String::new());
                continue;
            }
            K::If => {
                let rest = atom.name.strip_prefix("if:").unwrap_or("");
                values[i] = text(rest.to_string());
                continue;
            }
            K::AheadBehind => {
                let RefAtomArg::AheadBehind(base) = &atom.arg else { continue };
                let tip = match peel_to_commit_reporting(&item.oid) {
                    Ok(tip) => Some(tip),
                    Err(message) => {
                        if !reported_non_commit {
                            eprintln!("{}", message);
                            reported_non_commit = true;
                        }
                        None
                    }
                };
                let s = match tip {
                    Some(tip) => {
                        let (ahead, behind) = ahead_behind(&tip, base);
                        format!("{} {}", ahead, behind)
                    }
                    None => String::new(),
                };
                values[i] = text(s);
                continue;
            }
            _ => continue,
        };
        values[i] = text(if atom.deref { format!("{}^{{}}", refname) } else { refname });
    }

    let need_tagged = atoms.iter().any(|a| a.deref);
    let wants = |deref: bool| {
        atoms.iter().any(|a| {
            a.deref == deref
                && (a.needs_content || matches!(a.kind, RefAtomKind::ObjectType | RefAtomKind::ObjectSize | RefAtomKind::DeltaBase) || (deref && a.kind == RefAtomKind::ObjectName))
        })
    };
    if need_tagged || wants(false) || wants(true) {
        let (kind, content) = read_object(&item.oid).map_err(|_| format!("missing object {} for {}", item.oid, item.refname))?;
        grab_values(&mut values, atoms, false, &item.oid, &kind, &content)?;
        if need_tagged && kind == "tag" {
            if let Some(target) = tag_target(&content) {
                let (dkind, dcontent) = read_object(&target).map_err(|_| format!("missing object {} for {}", target, item.refname))?;
                grab_values(&mut values, atoms, true, &target, &dkind, &dcontent)?;
            }
        }
    }
    Ok(values.into_iter().map(Option::unwrap_or_default).collect())
}

fn show_oid(oid: &str, len: Option<usize>) -> String {
    match len {
        None => oid.to_string(),
        Some(n) => abbreviate_hash_min(oid, n),
    }
}

fn grab_values(values: &mut [Option<RefAtomValue>], atoms: &[RefFilterAtom], deref: bool, oid: &str, kind: &str, content: &[u8]) -> Result<(), String> {
    let parents: Vec<String> = content
        .split(|&b| b == b'\n')
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.strip_prefix(b"parent "))
        .map(|p| String::from_utf8_lossy(p).to_string())
        .collect();
    let set = |values: &mut [Option<RefAtomValue>], i: usize, s: Vec<u8>, value: u64| {
        values[i] = Some(RefAtomValue { s, value, binary: false });
    };
    for (i, atom) in atoms.iter().enumerate() {
        if atom.deref != deref {
            continue;
        }
        let bare = if deref { &atom.name[1..] } else { atom.name.as_str() };
        use RefAtomKind as K;
        match atom.kind {
            K::ObjectType => set(values, i, kind.as_bytes().to_vec(), 0),
            K::ObjectSize => {
                let size = if atom.arg == (RefAtomArg::ObjectSize { disk: true }) {
                    fs::metadata(crate::core::repo::path(format!(".git/objects/{}/{}", &oid[..2], &oid[2..]))).map(|m| m.len()).unwrap_or(0)
                } else {
                    content.len() as u64
                };
                set(values, i, size.to_string().into_bytes(), size);
            }
            K::DeltaBase => set(values, i, "0".repeat(40).into_bytes(), 0),
            K::ObjectName if deref => {
                let RefAtomArg::Oid(len) = &atom.arg else { continue };
                set(values, i, show_oid(oid, *len).into_bytes(), 0);
            }
            K::Raw => {
                if atom.arg == (RefAtomArg::Raw { size: true }) {
                    set(values, i, content.len().to_string().into_bytes(), content.len() as u64);
                } else {
                    values[i] = Some(RefAtomValue { s: content.to_vec(), value: 0, binary: true });
                }
            }
            _ => {}
        }
        if kind == "tag" {
            match atom.kind {
                K::Tag => set(values, i, header_value(content, "tag").unwrap_or_default().into_bytes(), 0),
                K::Type => {
                    if let Some(t) = header_value(content, "type") {
                        set(values, i, t.into_bytes(), 0);
                    }
                }
                K::Object => {
                    if let Some(t) = tag_target(content) {
                        set(values, i, t.into_bytes(), 0);
                    }
                }
                _ => {}
            }
        }
        if kind == "commit" {
            match atom.kind {
                K::Tree => {
                    let RefAtomArg::Oid(len) = &atom.arg else { continue };
                    set(values, i, show_oid(&header_value(content, "tree").unwrap_or_default(), *len).into_bytes(), 0);
                }
                K::NumParent => set(values, i, parents.len().to_string().into_bytes(), parents.len() as u64),
                K::Parent => {
                    let RefAtomArg::Oid(len) = &atom.arg else { continue };
                    let shown: Vec<String> = parents.iter().map(|p| show_oid(p, *len)).collect();
                    set(values, i, shown.join(" ").into_bytes(), 0);
                }
                K::Signature => {
                    let RefAtomArg::Signature(opt) = &atom.arg else { continue };
                    let s = match opt.as_str() {
                        "grade" => "N",
                        "trustlevel" => "undefined",
                        _ => "",
                    };
                    set(values, i, s.as_bytes().to_vec(), 0);
                }
                _ => {}
            }
        }
        if kind == "tag" || kind == "commit" {
            if bare == "body" || bare.starts_with("subject") || bare.starts_with("trailers") || bare.starts_with("contents") {
                if let RefAtomArg::Contents(part) = &atom.arg {
                    let (s, value) = grab_message_value(content, part);
                    set(values, i, s, value);
                }
            }
            if atom.kind == K::Describe {
                let RefAtomArg::Describe { tags, abbrev, matches, excludes } = &atom.arg else { continue };
                set(values, i, describe_value(oid, tags.unwrap_or(false), *abbrev, matches, excludes).into_bytes(), 0);
            }
        }
    }
    let people: &[&str] = match kind {
        "tag" => &["tagger"],
        "commit" => &["author", "committer"],
        _ => &[],
    };
    for who in people {
        grab_person(values, atoms, deref, who, content)?;
    }
    Ok(())
}

fn describe_value(oid: &str, all_tags: bool, abbrev: Option<usize>, matches: &[String], excludes: &[String]) -> String {
    let Some(commit) = peel_to_commit(oid) else { return String::new() };
    let accept = |name: &str| -> bool {
        if excludes.iter().any(|p| wildmatch(p, name, 0)) {
            return false;
        }
        matches.is_empty() || matches.iter().any(|p| wildmatch(p, name, 0))
    };
    match describe_commit_matching(&commit, all_tags, &accept) {
        Ok(Some((name, depth))) => {
            let abbrev = abbrev.unwrap_or(7);
            if depth == 0 || abbrev == 0 {
                name
            } else {
                format!("{}-{}-g{}", name, depth, abbreviate_hash_min(&commit, abbrev))
            }
        }
        _ => String::new(),
    }
}

fn find_wholine<'a>(who: &str, buf: &'a [u8]) -> &'a [u8] {
    let mut pos = 0;
    while pos < buf.len() {
        let line = &buf[pos..];
        if line.starts_with(who.as_bytes()) && line.get(who.len()) == Some(&b' ') {
            return &line[who.len() + 1..];
        }
        match line.iter().position(|&b| b == b'\n') {
            None => return b"",
            Some(eol) => {
                pos += eol + 1;
                if buf.get(pos) == Some(&b'\n') {
                    return b"";
                }
            }
        }
    }
    b""
}

fn line_of(buf: &[u8]) -> &[u8] {
    &buf[..buf.iter().position(|&b| b == b'\n').unwrap_or(buf.len())]
}

fn grab_person(values: &mut [Option<RefAtomValue>], atoms: &[RefFilterAtom], deref: bool, who: &str, buf: &[u8]) -> Result<(), String> {
    let wholine = find_wholine(who, buf);
    for (i, atom) in atoms.iter().enumerate() {
        if atom.deref != deref {
            continue;
        }
        let name = if deref { &atom.name[1..] } else { atom.name.as_str() };
        let Some(rest) = name.strip_prefix(who) else { continue };
        let s = if rest.is_empty() {
            line_of(wholine).to_vec()
        } else if rest.starts_with("name") {
            let line = line_of(wholine);
            match line.windows(2).position(|w| w == b" <") {
                Some(p) => line[..p].to_vec(),
                None => Vec::new(),
            }
        } else if rest.starts_with("email") {
            let RefAtomArg::Email { trim, localpart } = atom.arg else { continue };
            copy_email(wholine, trim, localpart)
        } else if rest.starts_with("date") {
            let (s, value) = grab_date(wholine, name)?;
            values[i] = Some(RefAtomValue { s, value, binary: false });
            continue;
        } else {
            continue;
        };
        values[i] = Some(RefAtomValue { s, value: 0, binary: false });
    }
    if who != "tagger" && who != "committer" {
        return Ok(());
    }
    for (i, atom) in atoms.iter().enumerate() {
        if atom.deref != deref {
            continue;
        }
        let name = if deref { &atom.name[1..] } else { atom.name.as_str() };
        match atom.kind {
            RefAtomKind::CreatorDate => {
                let (s, value) = grab_date(wholine, name)?;
                values[i] = Some(RefAtomValue { s, value, binary: false });
            }
            RefAtomKind::Creator => values[i] = Some(RefAtomValue { s: line_of(wholine).to_vec(), value: 0, binary: false }),
            _ => {}
        }
    }
    Ok(())
}

fn copy_email(buf: &[u8], trim: bool, localpart: bool) -> Vec<u8> {
    let Some(start) = buf.iter().position(|&b| b == b'<') else { return Vec::new() };
    let email = if trim || localpart { &buf[start + 1..] } else { &buf[start..] };
    let gt = email.iter().position(|&b| b == b'>');
    let end = if localpart {
        email.iter().position(|&b| b == b'@').or(gt)
    } else if trim {
        gt
    } else {
        gt.map(|p| p + 1)
    };
    match end {
        Some(e) => email[..e].to_vec(),
        None => Vec::new(),
    }
}

fn grab_date(buf: &[u8], atom_name: &str) -> Result<(Vec<u8>, u64), String> {
    let mode = match atom_name.find(':') {
        Some(p) => parse_ref_date_mode(&atom_name[p + 1..])?,
        None => RefDateMode::default(),
    };
    let Some(p) = buf.windows(2).position(|w| w == b"> ") else { return Ok((Vec::new(), 0)) };
    let rest = &buf[p + 2..];
    let digits_end = rest.iter().position(|b| !b.is_ascii_digit()).unwrap_or(rest.len());
    let Ok(timestamp) = String::from_utf8_lossy(&rest[..digits_end]).parse::<i64>() else { return Ok((Vec::new(), 0)) };
    let line_end = rest[digits_end..].iter().position(|&b| b == b'\n').map(|e| e + digits_end).unwrap_or(rest.len());
    let zone = String::from_utf8_lossy(&rest[digits_end..line_end]).to_string();
    let tz = parse_strtol_prefix(&zone);
    Ok((show_ref_date(timestamp, tz, &mode).into_bytes(), timestamp as u64))
}
