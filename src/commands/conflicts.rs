use crate::helpers::narrate::{conflict_kind, file_conflict_hunks, load_conflict_list, operation_in_progress, their_commit};

/// `rgit conflicts [--explain <file>...]`
///
/// rgit-only. Lists the files that still have unresolved conflicts: where the conflict
/// markers are, what kind of conflict it is, and who changed it last on each side; plus which
/// files are already resolved and the commands that continue or abort the operation.
/// `--explain <file>` prints every conflict of a file as YOURS / ORIGINAL / THEIRS blocks
/// with a one-line suggestion each.
pub fn conflicts(args: Vec<String>) -> anyhow::Result<()> {
    let entries = crate::index::read_index().unwrap_or_default();
    let mut unmerged: std::collections::BTreeMap<String, u8> = std::collections::BTreeMap::new();
    for e in entries.iter().filter(|e| e.stage != 0) {
        *unmerged.entry(e.path.clone()).or_default() |= 1 << (e.stage - 1);
    }
    let operation = operation_in_progress();
    let op = match operation.as_deref() {
        Some("merge") | Some("bisect") | None => "merge",
        Some(o) => o,
    };

    if let Some(pos) = args.iter().position(|a| a == "--explain") {
        let files: Vec<String> = if args.len() > pos + 1 { args[pos + 1..].to_vec() } else { unmerged.keys().cloned().collect() };
        if files.is_empty() {
            println!("No conflicts to explain: nothing is unresolved.");
        }
        for file in files {
            explain_file(&file, unmerged.get(&file).copied(), op);
        }
        return Ok(());
    }

    let recorded = load_conflict_list();
    let resolved: Vec<&String> = recorded.iter().filter(|p| !unmerged.contains_key(*p)).collect();
    if unmerged.is_empty() {
        match operation.as_deref() {
            Some("stash") => {
                println!("All conflicted files are resolved. The stash entry was kept: `rgit stash drop` when you're done with it.");
                crate::helpers::narrate::clear_stash_conflict();
            }
            Some(_) if !recorded.is_empty() => {
                println!("All {} conflicted files are resolved.", recorded.len());
                println!("Next: rgit {} --continue", op);
            }
            Some(o) => println!("No conflicts, but a {} is in progress: `rgit {} --continue` or `--abort`.", o, op),
            None => println!("No conflicts: nothing is unresolved."),
        }
        return Ok(());
    }
    let head = crate::refs::resolve_head_commit()?;
    let title = match operation.as_deref() {
        Some(o) if o != "bisect" => format!("{} in progress", o),
        _ => "Unresolved conflicts".into(),
    };
    println!("{}: {} left{}.", title, unmerged.len(), if recorded.is_empty() { String::new() } else { format!(" of {}", recorded.len()) });
    println!();
    for (path, mask) in &unmerged {
        for line in crate::ui::conflict_file_lines(path, *mask, operation.as_deref().unwrap_or("merge"), head.as_deref()) {
            println!("  {}", line);
        }
    }
    if !resolved.is_empty() {
        println!();
        println!("Resolved: {}", resolved.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
    }
    println!();
    println!("Next: edit each file, keep the code you want, delete the <<<<<<< ======= >>>>>>> lines,");
    for l in crate::ui::conflict_next_steps(operation.as_deref().unwrap_or("merge")) {
        println!("      {}", l);
    }
    println!("      `rgit conflicts --explain <file>` shows each conflict side by side.");
    Ok(())
}

/// Prints the conflicts of one file as YOURS / ORIGINAL / THEIRS blocks with a suggestion.
fn explain_file(path: &str, mask: Option<u8>, op: &str) {
    let hunks = file_conflict_hunks(path);
    if hunks.is_empty() {
        match mask {
            Some(m) => println!("{}: {} (no conflict markers in the file).\n  Keep it: `rgit add {}`. Drop it: `rgit rm {}`.\n", path, conflict_kind(m), path, path),
            None => println!("{}: no conflicts.\n", path),
        }
        return;
    }
    let _ = their_commit;
    for (i, h) in hunks.iter().enumerate() {
        println!("Conflict {} of {} in {} (lines {}-{}):", i + 1, hunks.len(), path, h.start, h.end);
        let block = |title: String, lines: &[String]| {
            println!("  {}", title);
            if lines.is_empty() {
                println!("    (nothing: this side removed these lines)");
            }
            for l in lines {
                println!("    | {}", l);
            }
        };
        block(format!("YOURS ({}):", if h.ours_label.is_empty() { "HEAD" } else { &h.ours_label }), &h.ours);
        match &h.base {
            Some(base) => block("ORIGINAL (before either change):".into(), base),
            None => println!("  ORIGINAL: not shown (`rgit config merge.conflictStyle diff3` adds it to future conflicts)"),
        }
        block(format!("THEIRS ({}):", if h.theirs_label.is_empty() { "other side" } else { &h.theirs_label }), &h.theirs);
        println!("  Suggestion: {}", suggestion(h));
        println!();
    }
    println!("When done: delete the marker lines, then `rgit add {}` (and `rgit {} --continue` once every file is resolved).\n", path, op);
}

/// A one-line hint about how the two sides of a conflict relate.
fn suggestion(h: &crate::objects::ConflictHunk) -> &'static str {
    let squash = |v: &[String]| v.iter().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect::<Vec<_>>();
    let (ours, theirs) = (squash(&h.ours), squash(&h.theirs));
    let contains = |big: &[String], small: &[String]| small.iter().all(|l| big.contains(l));
    if ours.is_empty() && theirs.is_empty() {
        "both sides only differ in blank lines: keep either."
    } else if ours == theirs {
        "the two sides differ only in whitespace: keep either."
    } else if ours.is_empty() {
        "you deleted these lines and they changed them: decide whether their change still matters."
    } else if theirs.is_empty() {
        "they deleted these lines and you changed them: decide whether your change still matters."
    } else if h.base.as_ref().is_some_and(|b| squash(b) == ours) {
        "you didn't really change this part, they did: keeping THEIRS is probably right."
    } else if h.base.as_ref().is_some_and(|b| squash(b) == theirs) {
        "they didn't really change this part, you did: keeping YOURS is probably right."
    } else if contains(&theirs, &ours) {
        "theirs keeps all of your lines and adds more: keeping THEIRS is probably right."
    } else if contains(&ours, &theirs) {
        "yours keeps all of their lines and adds more: keeping YOURS is probably right."
    } else {
        "both sides rewrote this part differently: combine them by hand."
    }
}
