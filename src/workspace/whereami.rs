pub mod cli;
pub mod engine;
pub mod types;

use crate::workspace::whereami::engine::{describe_operation, detached_for, gather, next_step};
use crate::workspace::whereami::types::Upstream;

/// `rgit whereami`: one screen on where the repository stands: branch and upstream (ahead /
/// behind as of the last fetch), last commit, uncommitted changes, the operation in progress,
/// stash entries and `rgit wip` saves, and the command most likely wanted next. Reads only.
pub fn whereami() -> anyhow::Result<()> {
    let s = gather()?;
    let short = |h: &str| h.get(..7).unwrap_or(h).to_string();
    let row = |label: &str, value: String| println!("{:<12}{}", label, value);

    match (&s.branch, &s.head) {
        (Some(b), Some(_)) => println!("On branch {}", b),
        (Some(b), None) => println!("On branch {} (no commits yet)", b),
        (None, h) => println!(
            "HEAD detached at {}{}",
            h.as_deref().map(short).unwrap_or_default(),
            detached_for(s.operation.as_deref()).map(|why| format!(" ({})", why)).unwrap_or_default()
        ),
    }
    if s.branch.is_some() {
        row("Upstream", match &s.upstream {
            Upstream::None(Some(remote)) => format!("none (not pushed to {} yet)", remote),
            Upstream::None(None) => "none (no remotes)".to_string(),
            Upstream::Gone(name) => format!("{} (gone: deleted on the remote)", name),
            Upstream::Tracking { name, ahead, behind } => format!("{}: {}", name, match (ahead, behind) {
                (0, 0) => "up to date".to_string(),
                (a, 0) => format!("{} ahead", a),
                (0, b) => format!("{} behind", b),
                (a, b) => format!("{} ahead, {} behind", a, b),
            }),
        });
    }
    if let Some(h) = &s.head
        && let Some((subject, author, when)) = crate::ui::narrate::commit_info(h)
    {
        row("Last commit", format!("{} {} ({}, {})", short(h), subject, author, crate::ui::narrate::relative_time(when)));
    }
    let mut changes: Vec<String> = Vec::new();
    for (n, what) in [(s.conflicted, "conflicted"), (s.staged, "staged"), (s.modified, "modified"), (s.untracked, "untracked")] {
        if n > 0 {
            changes.push(format!("{} {}", n, what));
        }
    }
    row("Changes", if changes.is_empty() { "none (clean)".to_string() } else { changes.join(", ") });
    if let Some(op) = &s.operation {
        row("In progress", describe_operation(op));
    }
    let mut saved: Vec<String> = Vec::new();
    if s.stash_count > 0 {
        saved.push(format!("{} stash entr{}", s.stash_count, if s.stash_count == 1 { "y" } else { "ies" }));
    }
    if s.wip_count > 0 {
        saved.push(format!("{} wip save{} (`rgit wip --list`)", s.wip_count, if s.wip_count == 1 { "" } else { "s" }));
    }
    if !saved.is_empty() {
        row("Saved", saved.join(", "));
    }
    row("Next", next_step(&s));
    Ok(())
}
