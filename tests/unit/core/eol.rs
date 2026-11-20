//! Unit tests for `src/core/eol.rs`, compiled into that module via `#[path]`.

use super::*;

const TRUE: Settings = Settings { autocrlf: AutoCrlf::True, eol_crlf: false };
const INPUT: Settings = Settings { autocrlf: AutoCrlf::Input, eol_crlf: false };
const OFF_LF: Settings = Settings { autocrlf: AutoCrlf::False, eol_crlf: false };
const OFF_CRLF: Settings = Settings { autocrlf: AutoCrlf::False, eol_crlf: true };

#[test]
fn test_action_for() {
    use EolAttr as E;
    use TextAttr as T;
    assert_eq!(action_for(&TRUE, T::Unspecified, E::Unspecified), Action::AutoCrlf);
    assert_eq!(action_for(&INPUT, T::Unspecified, E::Unspecified), Action::AutoInput);
    assert_eq!(action_for(&OFF_LF, T::Unspecified, E::Unspecified), Action::Binary);
    assert_eq!(action_for(&TRUE, T::Unset, E::Crlf), Action::Binary);
    assert_eq!(action_for(&OFF_LF, T::Unspecified, E::Crlf), Action::TextCrlf);
    assert_eq!(action_for(&TRUE, T::Set, E::Lf), Action::TextInput);
    assert_eq!(action_for(&OFF_CRLF, T::Auto, E::Unspecified), Action::AutoCrlf);
    assert_eq!(action_for(&OFF_LF, T::Auto, E::Unspecified), Action::AutoInput);
    assert_eq!(action_for(&OFF_CRLF, T::Set, E::Unspecified), Action::TextCrlf);
}

#[test]
fn test_conversions() {
    assert_eq!(crlf_to_lf(b"a\r\nb\rc\r\n"), b"a\nb\rc\n");
    assert_eq!(lf_to_crlf(b"a\nb\r\nc\n"), b"a\r\nb\r\nc\r\n");
    assert!(looks_binary(b"a\0b"));
    assert!(looks_binary(b"a\rb"));
    assert!(!looks_binary(b"a\r\nb\n"));
}

#[test]
fn test_clean_and_smudge() {
    // Auto: CRLF text is normalized unless the index copy already has CR.
    assert_eq!(clean_with(Action::AutoCrlf, b"a\r\nb\r\n".to_vec(), || false), b"a\nb\n");
    assert_eq!(clean_with(Action::AutoCrlf, b"a\r\nb\r\n".to_vec(), || true), b"a\r\nb\r\n");
    assert_eq!(clean_with(Action::AutoCrlf, b"a\0\r\n".to_vec(), || false), b"a\0\r\n");
    assert_eq!(clean_with(Action::Binary, b"a\r\n".to_vec(), || false), b"a\r\n");
    assert_eq!(clean_with(Action::TextInput, b"a\r\n".to_vec(), || true), b"a\n");
    // Smudge: auto skips content that already has CR or looks binary.
    assert_eq!(&*smudge_with(Action::AutoCrlf, b"a\nb\n"), b"a\r\nb\r\n");
    assert_eq!(&*smudge_with(Action::AutoCrlf, b"a\r\nb\n"), b"a\r\nb\n");
    assert_eq!(&*smudge_with(Action::AutoInput, b"a\n"), b"a\n");
    assert_eq!(&*smudge_with(Action::TextCrlf, b"a\r\nb\n"), b"a\r\nb\r\n");
}

#[test]
fn test_attributes() {
    let lines = parse_attributes("# comment\n*.sh eol=lf\n*.png binary\ndocs/*.txt text\n* text=auto\nfoo -diff\n");
    assert_eq!(lines.len(), 4);
    assert!(pattern_matches("*.sh", "scripts/run.sh"));
    assert!(pattern_matches("docs/*.txt", "docs/a.txt"));
    assert!(!pattern_matches("docs/*.txt", "x/docs/a.txt"));
    assert!(pattern_matches("/top.txt", "top.txt"));
}
