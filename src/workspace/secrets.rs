pub mod cli;
pub mod engine;
pub mod types;

use std::collections::{BTreeMap, BTreeSet};

use crate::core::object_store::{flatten_tree, tree_hash_of_commit};
use crate::workspace::secrets::engine::{added_lines, blob_text, scan_name, scan_text};
use crate::workspace::secrets::types::{Finding, Scope};

type Tree = BTreeMap<String, ([u8; 20], u32)>;

/// `rgit secrets [--all-files | --history [<rev>...]] [-q]`: looks for obvious secrets (cloud
/// and API keys, tokens, private keys, passwords in code or URLs, `.env`-style files) in the
/// staged changes (the lines they add: what a pre-commit hook needs), in every tracked file, or
/// in the lines each commit added. Matches are shown masked. Exit code 1 when anything is
/// found, so `exec rgit secrets` works as a pre-commit hook. A line containing
/// `rgit:allow-secret` is skipped.
pub fn secrets(scope: Scope, quiet: bool) -> anyhow::Result<()> {
    let head = crate::core::refs::resolve_head_commit()?;
    let mut head_tree = Tree::new();
    if let Some(h) = &head {
        flatten_tree(&tree_hash_of_commit(h)?, "", &mut head_tree)?;
    }
    let index: Tree = crate::core::index::read_index().unwrap_or_default().into_iter().filter(|e| e.stage == 0).map(|e| (e.path, (e.hash, e.mode))).collect();
    let mut found: Vec<Finding> = Vec::new();
    let what = match &scope {
        Scope::Staged => {
            for (path, (hash, _)) in &index {
                let old = head_tree.get(path);
                if old.map(|e| &e.0) == Some(hash) {
                    continue;
                }
                if old.is_none() {
                    found.extend(scan_name(path, None));
                }
                let Some(new) = blob_text(hash)? else { continue };
                let old_text = match old {
                    Some((h, _)) => blob_text(h)?,
                    None => None,
                };
                let lines = added_lines(old_text.as_deref(), &new);
                found.extend(scan_text(path, lines.iter().map(|(n, l)| (*n, l.as_str())), None));
            }
            "the staged changes"
        }
        Scope::AllFiles => {
            for (path, (hash, _)) in &index {
                found.extend(scan_name(path, None));
                if let Some(text) = blob_text(hash)? {
                    found.extend(scan_text(path, text.lines().enumerate().map(|(i, l)| (i + 1, l)), None));
                }
            }
            "the tracked files"
        }
        Scope::History(revs) => {
            if head.is_none() && revs.is_empty() {
                crate::ui::die_fatal("there are no commits yet");
            }
            let revs = if revs.is_empty() { vec!["HEAD".to_string()] } else { revs.clone() };
            let opts = crate::history::rev_list::engine::parse_revision_args(&revs)?;
            let (walked, _) = crate::history::rev_list::engine::select_rev_list_commits(&opts)?;
            // Oldest first, so each secret is reported at the commit that introduced it.
            let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
            for w in walked.iter().rev() {
                let c = &w.commit;
                if c.parents.len() > 1 {
                    continue;
                }
                let mut old = Tree::new();
                if let Some(p) = c.parents.first() {
                    flatten_tree(&tree_hash_of_commit(p)?, "", &mut old)?;
                }
                let mut new = Tree::new();
                flatten_tree(&c.tree, "", &mut new)?;
                for (path, (hash, _)) in &new {
                    let before = old.get(path);
                    if before.map(|e| &e.0) == Some(hash) {
                        continue;
                    }
                    let mut here: Vec<Finding> = Vec::new();
                    if before.is_none() {
                        here.extend(scan_name(path, Some(&c.hash)));
                    }
                    if let Some(text) = blob_text(hash)? {
                        let old_text = match before {
                            Some((h, _)) => blob_text(h)?,
                            None => None,
                        };
                        let lines = added_lines(old_text.as_deref(), &text);
                        here.extend(scan_text(path, lines.iter().map(|(n, l)| (*n, l.as_str())), Some(&c.hash)));
                    }
                    for f in here {
                        if seen.insert((f.path.clone(), f.what.clone(), f.masked.clone())) {
                            found.push(f);
                        }
                    }
                }
            }
            "the history"
        }
    };

    if !quiet {
        if found.is_empty() {
            println!("No secrets found in {}.", what);
        } else {
            println!("Possible secrets in {}:", what);
            let place = |f: &Finding| match f.line {
                Some(n) => format!("{}:{}", f.path, n),
                None => f.path.clone(),
            };
            let width = found.iter().map(|f| place(f).chars().count()).max().unwrap_or(0);
            let what_width = found.iter().map(|f| f.what.len()).max().unwrap_or(0);
            for f in &found {
                let commit = f.commit.as_ref().map(|c| format!("{}  ", &c[..7])).unwrap_or_default();
                let line = format!("  {}{:<w$}  {:<ww$}  {}", commit, place(f), f.what, f.masked, w = width, ww = what_width);
                println!("{}", line.trim_end());
            }
            println!();
            match scope {
                Scope::Staged => println!(
                    "{} found. Take {} out of the code (an environment variable, or a file listed in .gitignore); `rgit restore --staged <file>` unstages a file.",
                    found.len(),
                    if found.len() == 1 { "it" } else { "them" }
                ),
                Scope::AllFiles => println!("{} found. Move {} out of tracked files (`rgit ignore <file>` stops tracking a secrets file).", found.len(), if found.len() == 1 { "it" } else { "them" }),
                Scope::History(_) => println!("{} found in commits. Anyone with a clone has {}: revoke or rotate {}; deleting {} now doesn't remove {} from history.", found.len(), if found.len() == 1 { "it" } else { "them" }, if found.len() == 1 { "it" } else { "them" }, if found.len() == 1 { "it" } else { "them" }, if found.len() == 1 { "it" } else { "them" }),
            }
            println!("False alarm? Add `{}` in a comment on that line.", crate::workspace::secrets::engine::ALLOW_MARKER);
        }
    }
    if !found.is_empty() {
        crate::ui::exit(1);
    }
    Ok(())
}
