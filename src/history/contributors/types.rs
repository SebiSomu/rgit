//! Types of `rgit contributors`.

/// One contributor (after `.mailmap`): the email used most, commits authored, commits
/// co-authored (`Co-authored-by:` trailers), lines added and removed in the authored ones, and
/// the first and last commit time (author time, either role).
// Used by: src/history/contributors/types.rs, src/history/contributors.rs
// Used by command/helper: the `contributors` command
#[derive(Debug, Clone, Default)]
pub struct Contributor {
    pub emails: std::collections::BTreeMap<String, usize>,
    pub commits: usize,
    pub coauthored: usize,
    pub added: usize,
    pub removed: usize,
    pub first: i64,
    pub last: i64,
}

impl Contributor {
    /// The email this person used most (the alphabetically first on a tie).
    pub fn email(&self) -> String {
        let mut list: Vec<(&String, &usize)> = self.emails.iter().collect();
        list.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        list.first().map(|(e, _)| (*e).clone()).unwrap_or_default()
    }
}

/// How `rgit contributors` orders its table.
// Used by: src/history/contributors/types.rs, src/history/contributors/cli.rs, src/history/contributors.rs
// Used by command/helper: `dispatch_contributors` and the `contributors` command
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SortBy {
    Commits,
    Lines,
    First,
    Last,
    Name,
}
