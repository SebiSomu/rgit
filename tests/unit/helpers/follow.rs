//! Unit tests for `src/helpers/follow.rs`, kept outside `src/` and compiled into that module via
//! `#[path]` (so they can reach its private items through `super::`).

use crate::helpers::rename::pprint_rename;

#[test]
fn pprint_rename_matches_git() {
    assert_eq!(pprint_rename("a.txt", "b.txt"), "a.txt => b.txt");
    assert_eq!(pprint_rename("b.txt", "d/b2.txt"), "b.txt => d/b2.txt");
    assert_eq!(pprint_rename("src/a.rs", "src/b.rs"), "src/{a.rs => b.rs}");
    assert_eq!(pprint_rename("old/f.rs", "new/f.rs"), "{old => new}/f.rs");
    assert_eq!(pprint_rename("a/b/c", "a/x/c"), "a/{b => x}/c");
    assert_eq!(pprint_rename("a/b/c", "a/c"), "a/{b => }/c");
}
