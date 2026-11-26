//! Unit tests for `src/remote/credential/engine.rs`, compiled into that module via `#[path]`.

use super::*;

fn login(host: &str, user: &str, password: &str) -> Credential {
    Credential { protocol: Some("https".into()), host: Some(host.into()), username: Some(user.into()), password: Some(password.into()), ..Default::default() }
}

#[test]
fn store_get_and_erase_on_an_explicit_file() {
    let dir = tempfile::tempdir().unwrap();
    let files = vec![dir.path().join("creds")];
    store_action(&files, "store", &login("github.com", "sebi", "one")).unwrap();
    store_action(&files, "store", &login("gitlab.com", "me", "two")).unwrap();
    store_action(&files, "store", &login("github.com", "sebi", "three")).unwrap();
    assert_eq!(fs::read_to_string(&files[0]).unwrap(), "https://sebi:three@github.com\nhttps://me:two@gitlab.com\n");

    let answer = store_action(&files, "get", &Credential::for_https_host("github.com")).unwrap().unwrap();
    assert_eq!(answer.password.as_deref(), Some("three"));
    assert!(store_action(&files, "get", &Credential::for_https_host("example.com")).unwrap().is_none());

    // An incomplete login is not stored; erase with the wrong password keeps the entry.
    store_action(&files, "store", &Credential::for_https_host("example.com")).unwrap();
    store_action(&files, "erase", &login("github.com", "sebi", "wrong")).unwrap();
    assert!(fs::read_to_string(&files[0]).unwrap().contains("github.com"));
    store_action(&files, "erase", &Credential::for_https_host("github.com")).unwrap();
    assert_eq!(fs::read_to_string(&files[0]).unwrap(), "https://me:two@gitlab.com\n");
}

#[test]
fn store_writes_to_the_first_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let files = vec![dir.path().join("home-creds"), dir.path().join("xdg").join("credentials")];
    fs::create_dir_all(dir.path().join("xdg")).unwrap();
    fs::write(&files[1], "").unwrap();
    store_action(&files, "store", &login("github.com", "a", "b")).unwrap();
    assert!(!files[0].exists());
    assert_eq!(fs::read_to_string(&files[1]).unwrap(), "https://a:b@github.com\n");
}

#[test]
fn built_in_store_helper_takes_a_file_option() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("c");
    let helper = format!("store --file={}", file.display());
    run_helper(&helper, "store", &login("github.com", "a", "b"));
    let answer = run_helper(&helper, "get", &Credential::for_https_host("github.com")).unwrap();
    assert_eq!(answer.password.as_deref(), Some("b"));
    run_helper(&helper, "erase", &Credential::for_https_host("github.com"));
    assert!(run_helper(&helper, "get", &Credential::for_https_host("github.com")).is_none());
}

#[test]
fn store_options_are_parsed_like_git() {
    let (files, rest) = parse_store_options(&["--file".into(), "x".into(), "get".into()]).unwrap();
    assert_eq!(files, vec![PathBuf::from("x")]);
    assert_eq!(rest, vec!["get".to_string()]);
    assert!(parse_store_options(&["--bogus".into()]).is_err());
}

#[test]
fn credentials_without_host_or_protocol_are_refused() {
    assert!(check_required_fields(&Credential::default()).is_err());
    let mut no_protocol = Credential::for_https_host("h");
    no_protocol.protocol = None;
    assert_eq!(check_required_fields(&no_protocol).unwrap_err(), "refusing to work with credential missing protocol field");
}
