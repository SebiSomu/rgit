//! Sorting: `versioncmp` (with versionsort.suffix) and the `--sort` key comparison.

use crate::references::{
    for_each_ref::engine::output::c_string,
    for_each_ref::engine::upstream::config_values,
    for_each_ref::types::{RefArrayItem, RefAtomValue, RefFilterAtom},
};

/// The configured `versionsort.suffix` values (or the older `versionsort.prereleaseSuffix`,
/// with git's warning when both are set).
// Used by `for-each-ref` (`--sort=version:<key>`).
pub fn version_suffixes() -> Vec<String> {
    let new = config_values("versionsort.suffix");
    let old = config_values("versionsort.prereleasesuffix");
    if !new.is_empty() && !old.is_empty() {
        eprintln!("warning: ignoring versionsort.prereleasesuffix because versionsort.suffix is set");
    }
    if !new.is_empty() { new } else { old }
}

/// Compares two strings as version numbers like git's `versioncmp` (digit runs compare
/// numerically; `versionsort.suffix` values such as `-rc` sort before the release).
// Used by `for-each-ref` (`--sort=version:<key>`).
pub fn versioncmp(s1: &[u8], s2: &[u8], suffixes: &[String]) -> i32 {
    const S_N: usize = 0;
    const S_I: usize = 3;
    const S_F: usize = 6;
    const S_Z: usize = 9;
    const CMP: i32 = 2;
    const LEN: i32 = 3;
    const NEXT_STATE: [usize; 12] = [S_N, S_I, S_Z, S_N, S_I, S_I, S_N, S_F, S_F, S_N, S_F, S_Z];
    const RESULT: [i32; 36] = [
        CMP, CMP, CMP, CMP, LEN, CMP, CMP, CMP, CMP, //
        CMP, -1, -1, 1, LEN, LEN, 1, LEN, LEN, //
        CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, //
        CMP, 1, 1, -1, CMP, CMP, -1, CMP, CMP,
    ];
    let s1 = c_string(s1);
    let s2 = c_string(s2);
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);
    let class = |c: u8| usize::from(c == b'0') + usize::from(c.is_ascii_digit());
    let (mut p1, mut p2) = (0, 0);
    let mut c1 = at(s1, p1);
    let mut c2 = at(s2, p2);
    p1 += 1;
    p2 += 1;
    let mut state = S_N + class(c1);
    let mut diff;
    loop {
        diff = c1 as i32 - c2 as i32;
        if diff != 0 {
            break;
        }
        if c1 == 0 {
            return 0;
        }
        state = NEXT_STATE[state];
        c1 = at(s1, p1);
        c2 = at(s2, p2);
        p1 += 1;
        p2 += 1;
        state += class(c1);
    }
    if !suffixes.is_empty() {
        if let Some(d) = swap_prereleases(s1, s2, p1 - 1, suffixes) {
            return d;
        }
    }
    match RESULT[state * 3 + class(c2)] {
        CMP => diff,
        LEN => {
            loop {
                let a = at(s1, p1);
                p1 += 1;
                if !a.is_ascii_digit() {
                    break;
                }
                let b = at(s2, p2);
                p2 += 1;
                if !b.is_ascii_digit() {
                    return 1;
                }
            }
            if at(s2, p2).is_ascii_digit() { -1 } else { diff }
        }
        other => other,
    }
}

fn swap_prereleases(s1: &[u8], s2: &[u8], off: usize, suffixes: &[String]) -> Option<i32> {
    let find = |s: &[u8], suffix: &[u8], start: usize, conf_pos: i64, m: &mut (i64, usize, i64)| {
        let end = if m.2 < suffix.len() as i64 { m.1 as i64 } else { m.1 as i64 - 1 };
        let mut i = start as i64;
        while i <= end {
            if s.get(i as usize..).is_some_and(|r| r.starts_with(suffix)) {
                *m = (conf_pos, i as usize, suffix.len() as i64);
                break;
            }
            i += 1;
        }
    };
    let mut m1 = (-1i64, off, -1i64);
    let mut m2 = (-1i64, off, -1i64);
    for (i, suffix) in suffixes.iter().enumerate() {
        let suffix = suffix.as_bytes();
        let start = off.saturating_sub(suffix.len());
        find(s1, suffix, start, i as i64, &mut m1);
        find(s2, suffix, start, i as i64, &mut m2);
    }
    if m1.0 == -1 && m2.0 == -1 {
        return None;
    }
    if m1.0 == m2.0 {
        return None;
    }
    Some(if m1.0 >= 0 && m2.0 >= 0 {
        (m1.0 - m2.0) as i32
    } else if m1.0 >= 0 {
        -1
    } else {
        1
    })
}

fn strcasecmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let la = a.iter().map(|c| c.to_ascii_lowercase());
    let lb = b.iter().map(|c| c.to_ascii_lowercase());
    la.cmp(lb)
}

/// Orders two refs by the sort keys (primary first), like git's `compare_refs`: version,
/// string (case-insensitive with `--ignore-case`) or numeric comparison, `-` reversing, with
/// the ref name as the final tie-breaker.
// Used by `for-each-ref`.
pub fn compare_ref_items(
    a: (&RefArrayItem, &[RefAtomValue]),
    b: (&RefArrayItem, &[RefAtomValue]),
    sorts: &[(usize, bool, bool)],
    atoms: &[RefFilterAtom],
    ignore_case: bool,
    suffixes: &[String],
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    for &(at, reverse, version) in sorts {
        let (va, vb) = (&a.1[at], &b.1[at]);
        let cmp = if version {
            versioncmp(&va.s, &vb.s, suffixes).cmp(&0)
        } else if !atoms[at].numeric {
            if !va.binary && !vb.binary {
                if ignore_case { strcasecmp(c_string(&va.s), c_string(&vb.s)) } else { c_string(&va.s).cmp(c_string(&vb.s)) }
            } else if ignore_case {
                strcasecmp(&va.s, &vb.s)
            } else {
                va.s.cmp(&vb.s)
            }
        } else {
            va.value.cmp(&vb.value)
        };
        let cmp = if reverse { cmp.reverse() } else { cmp };
        if cmp != Ordering::Equal {
            return cmp;
        }
    }
    if ignore_case { strcasecmp(a.0.refname.as_bytes(), b.0.refname.as_bytes()) } else { a.0.refname.cmp(&b.0.refname) }
}
