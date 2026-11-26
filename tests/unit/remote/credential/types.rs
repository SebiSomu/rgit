//! Unit tests for `src/remote/credential/types.rs`, compiled into that module via `#[path]`.

use super::*;

#[test]
fn parses_protocol_lines_and_stops_at_blank_line() {
    let c = Credential::parse("protocol=https\nhost=github.com\nusername=me\npassword=a=b\n\nhost=ignored\n").unwrap();
    assert_eq!(c.protocol.as_deref(), Some("https"));
    assert_eq!(c.host.as_deref(), Some("github.com"));
    assert_eq!(c.password.as_deref(), Some("a=b"));
    assert!(Credential::parse("no equals sign\n").is_err());
    assert!(Credential::parse("capability[]=authtype\nhost=h\n").is_ok());
}

#[test]
fn url_input_is_split_and_decoded() {
    let c = Credential::parse("url=https://a%20b:p%40ss@example.com/team/repo.git\n").unwrap();
    assert_eq!(c.username.as_deref(), Some("a b"));
    assert_eq!(c.password.as_deref(), Some("p@ss"));
    assert_eq!(c.host.as_deref(), Some("example.com"));
    assert_eq!(c.path.as_deref(), Some("team/repo.git"));
    assert!(Credential::from_url("not a url").is_none());
}

#[test]
fn store_line_round_trips_with_git_encoding() {
    let c = Credential { protocol: Some("https".into()), host: Some("github.com".into()), username: Some("sebi".into()), password: Some("tok/en%1:@".into()), ..Default::default() };
    let line = c.to_store_line();
    assert_eq!(line, "https://sebi:tok%2fen%251%3a%40@github.com");
    assert_eq!(Credential::from_url(&line).unwrap().password, c.password);
}

#[test]
fn matching_only_checks_the_fields_asked_for() {
    let stored = Credential::from_url("https://sebi:x@github.com").unwrap();
    assert!(Credential::for_https_host("github.com").matches(&stored, false));
    let mut other_user = Credential::for_https_host("github.com");
    other_user.username = Some("someone".into());
    assert!(!other_user.matches(&stored, false));
    let mut with_path = Credential::for_https_host("github.com");
    with_path.path = Some("owner/repo".into());
    assert!(!with_path.matches(&stored, false));
    let mut wrong_password = Credential::for_https_host("github.com");
    wrong_password.password = Some("y".into());
    assert!(wrong_password.matches(&stored, false) && !wrong_password.matches(&stored, true));
}

#[test]
fn prompt_urls_include_the_user_for_passwords() {
    let mut c = Credential::for_https_host("github.com");
    assert_eq!(c.describe(true), "https://github.com");
    c.username = Some("me".into());
    assert_eq!(c.describe(false), "https://github.com");
    assert_eq!(c.describe(true), "https://me@github.com");
}
