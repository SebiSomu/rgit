pub mod cli;
pub mod types;

use std::collections::BTreeMap;

use crate::core::object_store::{flatten_tree, tree_hash_of_commit};
use crate::history::contributors::types::{Contributor, SortBy};

/// `rgit contributors [<rev>...] [--since <date>] [--sort <key>] [-e]`: everyone who wrote the
/// commits (non-merge, reachable from HEAD or the given revisions), with `.mailmap` applied:
/// commits authored and their share, commits co-authored (`Co-authored-by:` trailers), lines
/// added and removed, and the first and last commit day. Sorted by commits (or lines, first,
/// last, name).
pub fn contributors(revs: Vec<String>, since: Option<i64>, sort: SortBy, show_email: bool) -> anyhow::Result<()> {
    if crate::core::refs::resolve_head_commit()?.is_none() && revs.is_empty() {
        crate::ui::die_fatal("there are no commits yet");
    }
    let revs = if revs.is_empty() { vec!["HEAD".to_string()] } else { revs };
    let opts = crate::history::rev_list::engine::parse_revision_args(&revs)?;
    let (walked, _) = crate::history::rev_list::engine::select_rev_list_commits(&opts)?;
    let mailmap = crate::mail::mailmap::read_mailmap(None, None);
    let mut people: BTreeMap<String, Contributor> = BTreeMap::new();
    let mut total = 0usize;
    let touch = |people: &mut BTreeMap<String, Contributor>, name: &str, email: &str, when: i64| -> String {
        let (name, email) = crate::mail::mailmap::map_user(&mailmap, name, email);
        let c = people.entry(name.clone()).or_insert(Contributor { first: when, last: when, ..Default::default() });
        *c.emails.entry(email).or_default() += 1;
        c.first = c.first.min(when);
        c.last = c.last.max(when);
        name
    };
    for w in &walked {
        let commit = &w.commit;
        if commit.parents.len() > 1 || since.is_some_and(|s| commit.author.timestamp < s) {
            continue;
        }
        total += 1;
        let when = commit.author.timestamp;
        let name = touch(&mut people, &commit.author.name, &commit.author.email, when);
        let mut old = BTreeMap::new();
        if let Some(p) = commit.parents.first() {
            flatten_tree(&tree_hash_of_commit(p)?, "", &mut old)?;
        }
        let mut new = BTreeMap::new();
        flatten_tree(&commit.tree, "", &mut new)?;
        let (mut added, mut removed) = (0, 0);
        let mut paths: Vec<&String> = old.keys().chain(new.keys()).filter(|p| old.get(*p) != new.get(*p)).collect();
        paths.sort();
        paths.dedup();
        for p in paths {
            let (a, r) = crate::history::hotspots::engine::line_changes(old.get(p).map(|e: &([u8; 20], u32)| e.0), new.get(p).map(|e: &([u8; 20], u32)| e.0))?;
            added += a;
            removed += r;
        }
        let c = people.get_mut(&name).expect("just added");
        c.commits += 1;
        c.added += added;
        c.removed += removed;
        for line in commit.message.lines() {
            let Some(rest) = line.strip_prefix("Co-authored-by:").or_else(|| line.strip_prefix("Co-Authored-By:")) else { continue };
            if let Some((co_name, co_email)) = crate::mail::mailmap::split_contact(rest.trim()) {
                let co = touch(&mut people, &co_name, &co_email, when);
                if co != name {
                    people.get_mut(&co).expect("just added").coauthored += 1;
                }
            }
        }
    }
    if total == 0 {
        println!("No commits{}.", if since.is_some() { " in that time" } else { "" });
        return Ok(());
    }

    let mut list: Vec<(&String, &Contributor)> = people.iter().collect();
    list.sort_by(|a, b| {
        let key = match sort {
            SortBy::Commits => (b.1.commits + b.1.coauthored).cmp(&(a.1.commits + a.1.coauthored)),
            SortBy::Lines => (b.1.added + b.1.removed).cmp(&(a.1.added + a.1.removed)),
            SortBy::First => a.1.first.cmp(&b.1.first),
            SortBy::Last => b.1.last.cmp(&a.1.last),
            SortBy::Name => std::cmp::Ordering::Equal,
        };
        key.then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    println!("{} contributor{}, {} commit{}:", list.len(), if list.len() == 1 { "" } else { "s" }, total, if total == 1 { "" } else { "s" });
    let who = |name: &String, c: &Contributor| if show_email { format!("{} <{}>", name, c.email()) } else { name.clone() };
    let width = list.iter().map(|(n, c)| who(n, c).chars().count()).max().unwrap_or(0);
    let day = crate::history::story::engine::day;
    for (name, c) in &list {
        let co = if c.coauthored > 0 { format!(" (+{} co-authored)", c.coauthored) } else { String::new() };
        let range = if day(c.first) == day(c.last) { day(c.first) } else { format!("{} to {}", day(c.first), day(c.last)) };
        let commits = format!("{} commit{}{}", c.commits, if c.commits == 1 { "" } else { "s" }, co);
        println!("  {:<w$}  {:<26}  {:>3}%  {:<15}  {}", who(name, c), commits, c.commits * 100 / total, format!("+{} -{}", c.added, c.removed), range, w = width);
    }
    Ok(())
}
