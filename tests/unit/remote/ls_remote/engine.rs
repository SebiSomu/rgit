//! Unit tests for `src/remote/ls_remote/engine.rs`, compiled into that module via `#[path]`.

use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parses_flags_repository_and_patterns() {
    let o = parse_ls_remote_args(&args(&["--heads", "-t", "--refs", "--symref", "-q", "--exit-code", "origin", "main", "v*"])).unwrap();
    assert!(o.heads && o.tags && o.refs_only && o.symref && o.quiet && o.exit_code && !o.get_url);
    assert_eq!(o.repository.as_deref(), Some("origin"));
    assert_eq!(o.patterns, vec!["main".to_string(), "v*".to_string()]);
    let o = parse_ls_remote_args(&args(&["-qt", "--upload-pack=x", "--", "-weird-name"])).unwrap();
    assert!(o.quiet && o.tags);
    assert_eq!(o.repository.as_deref(), Some("-weird-name"));
}

#[test]
fn a_lone_dash_h_is_help_but_otherwise_means_heads() {
    assert_eq!(parse_ls_remote_args(&args(&["-h"])).unwrap_err(), USAGE);
    assert!(parse_ls_remote_args(&args(&["-h", "origin"])).unwrap().heads);
    assert!(parse_ls_remote_args(&args(&["--nope"])).unwrap_err().starts_with("error: unknown option `nope'"));
    assert!(parse_ls_remote_args(&args(&["-x"])).unwrap_err().starts_with("error: unknown switch `x'"));
}

fn r(name: &str, oid: &str, symref: Option<&str>, peeled: Option<&str>) -> RemoteRef {
    RemoteRef { name: name.into(), oid: Some(oid.into()), symref_target: symref.map(Into::into), peeled: peeled.map(Into::into) }
}

fn sample() -> Vec<RemoteRef> {
    vec![
        r("HEAD", "aaaa", Some("refs/heads/main"), None),
        r("refs/heads/main", "aaaa", None, None),
        r("refs/heads/feat/x", "bbbb", None, None),
        r("refs/tags/v1", "cccc", None, Some("aaaa")),
        RemoteRef { name: "refs/heads/unborn".into(), oid: None, symref_target: None, peeled: None },
    ]
}

fn names(options: LsRemoteOptions) -> Vec<String> {
    select_refs(&sample(), &options).into_iter().map(|r| r.name).collect()
}

#[test]
fn filters_follow_gits_check_ref() {
    let all = names(LsRemoteOptions::default());
    assert_eq!(all, ["HEAD", "refs/heads/main", "refs/heads/feat/x", "refs/tags/v1", "refs/tags/v1^{}"], "unborn refs are not listed");
    assert_eq!(names(LsRemoteOptions { heads: true, ..Default::default() }), ["refs/heads/main", "refs/heads/feat/x"]);
    assert_eq!(names(LsRemoteOptions { tags: true, ..Default::default() }), ["refs/tags/v1", "refs/tags/v1^{}"]);
    assert_eq!(names(LsRemoteOptions { refs_only: true, ..Default::default() }), ["refs/heads/main", "refs/heads/feat/x", "refs/tags/v1"]);
    assert_eq!(names(LsRemoteOptions { tags: true, refs_only: true, ..Default::default() }), ["refs/tags/v1"]);
}

#[test]
fn patterns_match_the_end_of_the_name() {
    let with = |p: &[&str]| names(LsRemoteOptions { patterns: p.iter().map(|s| s.to_string()).collect(), ..Default::default() });
    assert_eq!(with(&["main"]), ["refs/heads/main"]);
    assert_eq!(with(&["HEAD"]), ["HEAD"]);
    assert_eq!(with(&["v1"]), ["refs/tags/v1"], "the ^{{}} entry has its own name");
    assert_eq!(with(&["feat*"]), ["refs/heads/feat/x"]);
    assert_eq!(with(&["ain"]), Vec::<String>::new(), "a pattern matches whole path components");
}

#[test]
fn formats_like_git() {
    let listed = select_refs(&sample()[..1], &LsRemoteOptions::default());
    assert_eq!(format_listing(&listed, false), "aaaa\tHEAD\n");
    assert_eq!(format_listing(&listed, true), "ref: refs/heads/main\tHEAD\naaaa\tHEAD\n");
}
