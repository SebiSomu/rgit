pub mod cli;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Context;
use crate::workspace::gitignore::{collect_files, normalize_path};
use crate::core::object_store::{flatten_tree, hash_content, tree_hash_of_commit, write_object};
use crate::core::index::{build_entry, read_index, write_index};
use crate::core::refs;

pub fn add(paths: Vec<PathBuf>) -> anyhow::Result<()> {
    let mut entries = read_index().unwrap_or_default();
    let mut files_to_add: Vec<PathBuf> = Vec::new();
    // Submodules: a directory that is a repository of its own is staged as a gitlink (the
    // commit it has checked out), and tracked submodules below the given paths are refreshed.
    let mut gitlinks_changed = 0;
    for path in &paths {
        let given = normalize_path(path);
        let mut candidates: Vec<String> = entries.iter().filter(|e| e.mode == 0o160000 && (given.is_empty() || given == "." || e.path == given || e.path.starts_with(&format!("{}/", given)))).map(|e| e.path.clone()).collect();
        if !given.is_empty() && given != "." && path.join(".git").exists() && !candidates.contains(&given) {
            candidates.push(given.clone());
        }
        for sub_path in candidates {
            let Some(head) = crate::workspace::submodule::engine::submodule_head(&sub_path) else { continue };
            let mut hash = [0u8; 20];
            hex::decode_to_slice(&head, &mut hash)?;
            if entries.iter().any(|e| e.path == sub_path && e.hash == hash && e.mode == 0o160000) {
                continue;
            }
            let below = format!("{}/", sub_path);
            entries.retain(|e| e.path != sub_path && !e.path.starts_with(&below));
            entries.push(crate::workspace::index_plumbing::bare_index_entry(&sub_path, hash, 0o160000));
            gitlinks_changed += 1;
        }
    }
    if gitlinks_changed > 0 {
        write_index(&mut entries)?;
    }
    // A tracked path that no longer exists on disk is staged as a deletion (this is how a
    // conflict is resolved by removing the file); its unmerged stages go with it.
    let mut removed = 0;
    for path in &paths {
        let given = normalize_path(path);
        if path.exists() || given.is_empty() || given == "." {
            continue;
        }
        let below = format!("{}/", given);
        let before = entries.len();
        let gone: std::collections::BTreeSet<String> = entries.iter().filter(|e| e.path == given || e.path.starts_with(&below)).map(|e| e.path.clone()).collect();
        entries.retain(|e| e.path != given && !e.path.starts_with(&below));
        if entries.len() != before {
            removed += gone.len();
        }
    }
    if removed > 0 {
        write_index(&mut entries)?;
    }
    for path in paths {
        if !path.exists() && removed > 0 {
            continue;
        }
        // The submodule itself was handled above; its files are not ours.
        if path.is_dir() && path.join(".git").exists() && !normalize_path(&path).is_empty() && normalize_path(&path) != "." {
            continue;
        }
        collect_files(&path, &mut files_to_add)?;
    }

    if files_to_add.is_empty() && removed > 0 {
        println!("Staged files to index ({} removed).", removed);
        return Ok(());
    }
    if files_to_add.is_empty() && gitlinks_changed > 0 {
        println!("Staged files to index ({} submodule{}).", gitlinks_changed, if gitlinks_changed == 1 { "" } else { "s" });
        return Ok(());
    }
    if files_to_add.is_empty() {
        println!("nothing specified, nothing added.");
        return Ok(());
    }

    let mut added = 0;
    let mut modified = 0;
    let mut unchanged = 0;

    for file_path in files_to_add {
        let rel_path = normalize_path(&file_path);
        let content = fs::read(&file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;
        let content = crate::core::eol::clean(&rel_path, content);

        let hash = hash_content("blob", &content);

        // An unmerged path always gets its stages replaced by the new stage-0 entry.
        let conflicted = entries.iter().any(|e| e.path == rel_path && e.stage != 0);
        match entries.iter().find(|e| e.path == rel_path && e.stage == 0) {
            _ if conflicted => modified += 1,
            Some(existing) if existing.hash == hash => {
                // Content already matches what's staged -- nothing to do.
                unchanged += 1;
                continue;
            }
            Some(_) => modified += 1,
            None => added += 1,
        }

        // Only now actually write the object, since we know it's new content.
        write_object("blob", &content)?;

        let metadata = fs::metadata(&file_path)?;
        let new_entry = build_entry(&rel_path, hash, &metadata);

        entries.retain(|e| e.path != rel_path);
        entries.push(new_entry);
    }

    if added == 0 && modified == 0 && removed == 0 {
        println!(
            "nothing to add: {} file(s) already up to date, no modifications found",
            unchanged
        );
        return Ok(());
    }

    write_index(&mut entries)?;

    let mut summary = Vec::new();
    if added > 0 {
        summary.push(format!("{} new", added));
    }
    if modified > 0 {
        summary.push(format!("{} modified", modified));
    }
    if removed > 0 {
        summary.push(format!("{} removed", removed));
    }
    if unchanged > 0 {
        summary.push(format!("{} unchanged", unchanged));
    }
    println!("Staged files to index ({}).", summary.join(", "));

    Ok(())
}

pub fn status(opts: crate::workspace::status::types::StatusOptions) -> anyhow::Result<()> {
    let short = opts.short;
    // The long format's header lines; the short format prints none of them.
    macro_rules! say { ($($t:tt)*) => { if !short { println!($($t)*); } } }
    let head_state = refs::resolve_head()?;

    let mut no_commits_yet = false;
    let mut head_tree: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();

    match &head_state {
        refs::HeadState::Branch(branch_name) => {
            say!("On branch {}", branch_name);
            let ref_path = format!("refs/heads/{}", branch_name);
            if let Some(commit_hash) = refs::read_ref(&ref_path)? {
                let tree_hash = tree_hash_of_commit(&commit_hash)?;
                flatten_tree(&tree_hash, "", &mut head_tree)?;
            } else {
                no_commits_yet = true;
                say!("\nNo commits yet");
            }
        }
        refs::HeadState::Detached(hash) => {
            say!("HEAD detached at {}", &hash[..7]);
            let tree_hash = tree_hash_of_commit(hash)?;
            flatten_tree(&tree_hash, "", &mut head_tree)?;
        }
    }

    let index_entries = read_index().unwrap_or_default();
    let index_map: BTreeMap<String, ([u8; 20], u32)> = index_entries
        .iter()
        .filter(|e| e.stage == 0)
        .map(|e| (e.path.clone(), (e.hash, e.mode)))
        .collect();
    // Unmerged paths and which of stages 1-3 each has (bit `stage - 1`).
    let mut unmerged: BTreeMap<String, u8> = BTreeMap::new();
    for entry in index_entries.iter().filter(|e| e.stage != 0) {
        *unmerged.entry(entry.path.clone()).or_default() |= 1 << (entry.stage - 1);
    }

    // The in-progress operation, as git reports it (with status hints off).
    let mut state_lines: Vec<String> = Vec::new();
    if crate::core::repo::path(".git/MERGE_HEAD").exists() {
        state_lines.push(if unmerged.is_empty() { "All conflicts fixed but you are still merging.".into() } else { "You have unmerged paths.".into() });
    } else if let Ok(pick) = fs::read_to_string(crate::core::repo::path(".git/CHERRY_PICK_HEAD")) {
        state_lines.push(format!("You are currently cherry-picking commit {}.", pick.trim().get(..7).unwrap_or(pick.trim())));
    } else if let Ok(revert) = fs::read_to_string(crate::core::repo::path(".git/REVERT_HEAD")) {
        state_lines.push(format!("You are currently reverting commit {}.", revert.trim().get(..7).unwrap_or(revert.trim())));
    }
    for line in &state_lines {
        say!("{}", line);
    }
    if !state_lines.is_empty() {
        say!();
    }

    if crate::workspace::sparse_checkout::engine::sparse_checkout_enabled() && !index_entries.is_empty() {
        let present = index_entries.iter().filter(|e| !e.skip_worktree).count();
        say!("You are in a sparse checkout with {}% of tracked files present.", 100 - (index_entries.len() - present) * 100 / index_entries.len());
    }

    let mut staged_new = Vec::new();
    let mut staged_modified = Vec::new();
    let mut staged_deleted = Vec::new();

    for (path, (hash, mode)) in &index_map {
        match head_tree.get(path) {
            None => staged_new.push(path.clone()),
            Some((head_hash, head_mode)) if head_hash != hash || head_mode != mode => staged_modified.push(path.clone()),
            _ => {}
        }
    }
    for path in head_tree.keys() {
        if !index_map.contains_key(path) && !unmerged.contains_key(path) {
            staged_deleted.push(path.clone());
        }
    }
    // Staged renames and copies (`status.renames`, else `diff.renames`; on by default).
    let mut staged_renamed: Vec<(char, String, String)> = Vec::new();
    let rename_opts = crate::diff::rename::default_rename_options("status.renames");
    if rename_opts.level > 0 && !staged_new.is_empty() {
        use crate::diff::types::{DiffChange, DiffSide};
        let side = |e: &([u8; 20], u32)| DiffSide { mode: e.1, hash: e.0, worktree: false };
        let paths: std::collections::BTreeSet<&String> = head_tree.keys().chain(index_map.keys()).collect();
        let changes: Vec<DiffChange> = paths
            .into_iter()
            .filter(|p| head_tree.get(*p) != index_map.get(*p) && !unmerged.contains_key(*p))
            .map(|p| DiffChange { path: p.clone(), old: head_tree.get(p).map(side), new: index_map.get(p).map(side), renamed_from: None, unmerged: false })
            .collect();
        let unmodified = if rename_opts.find_copies_harder { crate::diff::plumbing::unmodified_files(&head_tree, &changes) } else { Vec::new() };
        for change in crate::diff::rename::detect_renames(changes, &rename_opts, &unmodified) {
            if let Some((source, status, _)) = change.renamed_from {
                staged_new.retain(|p| *p != change.path);
                if status == 'R' {
                    staged_deleted.retain(|p| *p != source);
                }
                staged_renamed.push((status, source, change.path));
            }
        }
    }

    let mut working_files = Vec::new();
    collect_files(Path::new("."), &mut working_files)?;

    let mut working_map: BTreeMap<String, [u8; 20]> = BTreeMap::new();
    for file_path in &working_files {
        let rel_path = normalize_path(file_path);
        let content = fs::read(file_path).with_context(|| format!("Failed to read {}", file_path.display()))?;
        let content = crate::core::eol::clean(&rel_path, content);
        working_map.insert(rel_path, hash_content("blob", &content));
    }

    for path in index_map.keys() {
        if !working_map.contains_key(path) {
            let path_obj = Path::new(path);
            if path_obj.exists() && path_obj.is_file() {
                if let Ok(content) = crate::core::eol::read_clean(path) {
                    working_map.insert(path.clone(), hash_content("blob", &content));
                }
            }
        }
    }

    let mut unstaged_modified = Vec::new();
    let mut unstaged_deleted = Vec::new();
    let mut untracked = Vec::new();

    for (path, hash) in &working_map {
        if unmerged.contains_key(path) {
            continue;
        }
        match index_map.get(path) {
            None => untracked.push(path.clone()),
            Some((idx_hash, _)) if idx_hash != hash => unstaged_modified.push(path.clone()),
            _ => {}
        }
    }
    // Files left out by a sparse checkout (skip-worktree) are absent on purpose.
    let skipped = crate::workspace::sparse_checkout::engine::skipped_worktree_paths(&index_entries);
    for (path, (hash, mode)) in &index_map {
        if *mode == 0o160000 {
            // A submodule: changed when its checked-out commit differs from the staged one.
            if crate::workspace::submodule::engine::submodule_head(path).is_some_and(|head| head != hex::encode(hash)) {
                unstaged_modified.push(format!("{} (new commits)", path));
            }
            continue;
        }
        if !working_map.contains_key(path) && !skipped.contains(path) {
            unstaged_deleted.push(path.clone());
        }
    }

    // `-u`: no untracked files, git's collapsed directories (default), or every file.
    let untracked = match opts.untracked.as_str() {
        "no" => Vec::new(),
        "all" => untracked,
        _ => {
            let tracked: std::collections::BTreeSet<String> = index_entries.iter().map(|e| e.path.clone()).collect();
            crate::workspace::status::collapse_untracked(&untracked, &tracked)
        }
    };

    if short {
        let mut staged: Vec<(char, String)> = Vec::new();
        staged.extend(staged_new.iter().map(|p| ('A', p.clone())));
        staged.extend(staged_modified.iter().map(|p| ('M', p.clone())));
        staged.extend(staged_deleted.iter().map(|p| ('D', p.clone())));
        let mut unstaged: Vec<(char, String)> = Vec::new();
        unstaged.extend(unstaged_modified.iter().map(|p| ('M', p.strip_suffix(" (new commits)").unwrap_or(p).to_string())));
        unstaged.extend(unstaged_deleted.iter().map(|p| ('D', p.clone())));
        let header = opts.branch.then(|| crate::workspace::status::status_branch_header(&head_state, no_commits_yet));
        print!("{}", crate::workspace::status::format_short_status(&opts, header, &staged, &staged_renamed, &unstaged, &unmerged, &untracked));
        return Ok(());
    }

    let has_staged = !staged_new.is_empty() || !staged_modified.is_empty() || !staged_deleted.is_empty() || !staged_renamed.is_empty();
    let has_unstaged = !unstaged_modified.is_empty() || !unstaged_deleted.is_empty();
    let has_untracked = !untracked.is_empty();
    let mut need_leading_blank = no_commits_yet;

    // Each section lists its paths in path order, whatever kind of change they are (a rename
    // sorts by its new path), like git.
    if has_staged {
        if need_leading_blank { println!(); }
        println!("Changes to be committed:");
        let mut lines: Vec<(String, String)> = Vec::new();
        lines.extend(staged_new.iter().map(|p| (p.clone(), format!("new file:   {}", p))));
        lines.extend(staged_modified.iter().map(|p| (p.clone(), format!("modified:   {}", p))));
        lines.extend(staged_deleted.iter().map(|p| (p.clone(), format!("deleted:    {}", p))));
        for (status, from, to) in &staged_renamed {
            lines.push((to.clone(), format!("{}{} -> {}", if *status == 'R' { "renamed:    " } else { "copied:     " }, from, to)));
        }
        lines.sort();
        for (_, line) in &lines { println!("\t{}", line); }
        println!();
        need_leading_blank = false;
    }

    if !unmerged.is_empty() {
        if need_leading_blank { println!(); }
        println!("Unmerged paths:");
        for (path, mask) in &unmerged {
            let how = match mask {
                1 => "both deleted:",
                2 => "added by us:",
                3 => "deleted by them:",
                4 => "added by them:",
                5 => "deleted by us:",
                6 => "both added:",
                _ => "both modified:",
            };
            // Labels are padded to the longest one ("deleted by them:") plus a space.
            println!("\t{:<17}{}", how, path);
        }
        println!();
        need_leading_blank = false;
    }

    if has_unstaged {
        if need_leading_blank { println!(); }
        println!("Changes not staged for commit:");
        let mut lines: Vec<(String, String)> = Vec::new();
        lines.extend(unstaged_modified.iter().map(|p| (p.clone(), format!("modified:   {}", p))));
        lines.extend(unstaged_deleted.iter().map(|p| (p.clone(), format!("deleted:    {}", p))));
        lines.sort();
        for (_, line) in &lines { println!("\t{}", line); }
        println!();
        need_leading_blank = false;
    }

    if has_untracked {
        if need_leading_blank { println!(); }
        println!("Untracked files:");
        for p in &untracked { println!("\t{}", p); }
        println!();
        need_leading_blank = false;
    }

    if opts.untracked == "no" {
        if need_leading_blank { println!(); }
        println!("Untracked files not listed (use -u option to show untracked files)");
        need_leading_blank = false;
    }

    if !has_staged {
        if need_leading_blank { println!(); }
        // git's trailers (with its status hints).
        if no_commits_yet && !has_unstaged && !has_untracked {
            println!("nothing to commit (create/copy files and use \"rgit add\" to track)");
        } else if has_unstaged || !unmerged.is_empty() {
            println!("no changes added to commit (use \"rgit add\" and/or \"rgit commit -a\")");
        } else if has_untracked {
            println!("nothing added to commit but untracked files present (use \"rgit add\" to track)");
        } else if opts.untracked == "no" {
            println!("nothing to commit (use -u to show untracked files)");
        } else {
            println!("nothing to commit, working tree clean");
        }
    }

    Ok(())
}

pub fn rm(files: Vec<PathBuf>, force: bool, cached: bool, recursive: bool) -> anyhow::Result<()> {
    if files.is_empty() {
        anyhow::bail!("fatal: No pathspec was given. Which files should I remove?");
    }

    let mut index_entries = read_index().unwrap_or_default();
    let head_tree = if let Some(commit_hash) = refs::resolve_head_commit()? {
        let tree_hash = tree_hash_of_commit(&commit_hash)?;
        let mut map = BTreeMap::new();
        flatten_tree(&tree_hash, "", &mut map)?;
        Some(map)
    } else {
        None
    };

    let mut paths_to_remove: Vec<String> = Vec::new();

    for path_buf in &files {
        let rel_path = normalize_path(path_buf);

        let matching_entries: Vec<String> = index_entries
            .iter()
            .filter_map(|e| {
                if e.path == rel_path {
                    Some(e.path.clone())
                } else if recursive && e.path.starts_with(&format!("{}/", rel_path)) {
                    Some(e.path.clone())
                } else {
                    None
                }
            })
            .collect();

        if matching_entries.is_empty() {
            let has_subentries = index_entries
                .iter()
                .any(|e| e.path.starts_with(&format!("{}/", rel_path)));

            if has_subentries && !recursive {
                anyhow::bail!("fatal: not removing '{}' recursively without -r", rel_path);
            } else {
                anyhow::bail!("fatal: pathspec '{}' did not match any files", rel_path);
            }
        }

        for path in matching_entries {
            if !paths_to_remove.contains(&path) {
                paths_to_remove.push(path);
            }
        }
    }

    if !force {
        let mut staged_files = Vec::new();
        let mut modified_files = Vec::new();

        for path in &paths_to_remove {
            // An unmerged path is removed without the up-to-date checks, as in git.
            if index_entries.iter().any(|e| &e.path == path && e.stage != 0) {
                continue;
            }
            let entry = index_entries.iter().find(|e| &e.path == path);
            let idx_hash = entry.map(|e| e.hash);

            let head_hash = head_tree.as_ref().and_then(|t| t.get(path)).map(|(h, _)| *h);

            let index_staged = match (idx_hash, head_hash) {
                (Some(ih), Some(hh)) => ih != hh,
                (Some(_), None) => true,
                _ => false,
            };

            let disk_modified = if !cached && !index_staged && Path::new(path).exists() {
                if let Ok(content) = crate::core::eol::read_clean(path) {
                    let disk_hash = hash_content("blob", &content);
                    idx_hash.map(|ih| disk_hash != ih).unwrap_or(false)
                } else {
                    false
                }
            } else {
                false
            };

            // Mirror git's own precedence: a file that differs from HEAD is reported
            // as having staged changes, even if it also has further working-tree edits.
            if index_staged {
                staged_files.push(path.clone());
            } else if disk_modified {
                modified_files.push(path.clone());
            }
        }

        if !staged_files.is_empty() || !modified_files.is_empty() {
            let mut msg = String::new();

            if !staged_files.is_empty() {
                msg.push_str("error: the following file has changes staged in the index:\n");
                for file in &staged_files {
                    msg.push_str(&format!("    {}\n", file));
                }
            }
            if !modified_files.is_empty() {
                msg.push_str("error: the following file has local modifications:\n");
                for file in &modified_files {
                    msg.push_str(&format!("    {}\n", file));
                }
            }
            msg.push_str("(use --cached to keep the file, or -f to force removal)");
            anyhow::bail!(msg);
        }
    }

    for path in &paths_to_remove {
        index_entries.retain(|e| &e.path != path);

        if !cached {
            let path_obj = Path::new(path);
            if path_obj.exists() {
                fs::remove_file(path_obj)
                    .with_context(|| format!("failed to remove file '{}'", path))?;

                let mut parent = path_obj.parent();
                while let Some(p) = parent {
                    if p == Path::new("") || p == Path::new(".") {
                        break;
                    }
                    if p.exists() {
                        if fs::read_dir(p)?.next().is_none() {
                            fs::remove_dir(p)?;
                        } else {
                            break;
                        }
                    }
                    parent = p.parent();
                }
            }
        }

        println!("rm '{}'", path);
    }

    write_index(&mut index_entries)?;

    Ok(())
}

pub fn mv(source: PathBuf, destination: PathBuf, force: bool) -> anyhow::Result<()> {
    let src_rel = normalize_path(&source);

    let mut index_entries = read_index().unwrap_or_default();
    let (src_hash, src_mode) = index_entries
        .iter()
        .find(|e| e.path == src_rel)
        .map(|e| (e.hash, e.mode))
        .ok_or_else(|| anyhow::anyhow!("fatal: not under version control: '{}'", src_rel))?;

    if !Path::new(&src_rel).exists() {
        anyhow::bail!("fatal: bad source, source={} is missing from the working tree", src_rel);
    }

    let dest_rel = if Path::new(&destination).is_dir() {
        let file_name = Path::new(&src_rel)
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("fatal: '{}' has no file name component", src_rel))?;
        normalize_path(&Path::new(&destination).join(file_name))
    } else {
        normalize_path(&destination)
    };

    if src_rel == dest_rel {
        anyhow::bail!("fatal: '{}' and '{}' are the same file", src_rel, dest_rel);
    }

    let dest_already_tracked = index_entries.iter().any(|e| e.path == dest_rel);
    let dest_exists_on_disk = Path::new(&dest_rel).exists();

    if (dest_already_tracked || dest_exists_on_disk) && !force {
        anyhow::bail!(
            "fatal: destination '{}' already exists; use -f to overwrite",
            dest_rel
        );
    }

    if let Some(parent) = Path::new(&dest_rel).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory for '{}'", dest_rel))?;
        }
    }

    fs::rename(&src_rel, &dest_rel)
        .with_context(|| format!("failed to rename '{}' to '{}'", src_rel, dest_rel))?;

    let mut parent = Path::new(&src_rel).parent();
    while let Some(p) = parent {
        if p == Path::new("") || p == Path::new(".") {
            break;
        }
        if p.exists() {
            if fs::read_dir(p)?.next().is_none() {
                fs::remove_dir(p)?;
            } else {
                break;
            }
        }
        parent = p.parent();
    }

    index_entries.retain(|e| e.path != dest_rel && e.path != src_rel);

    let metadata = fs::metadata(&dest_rel)
        .with_context(|| format!("failed to stat '{}' after move", dest_rel))?;
    let mut new_entry = build_entry(&dest_rel, src_hash, &metadata);
    new_entry.mode = src_mode;
    index_entries.push(new_entry);

    write_index(&mut index_entries)?;

    println!("renamed '{}' -> '{}'", src_rel, dest_rel);

    Ok(())
}