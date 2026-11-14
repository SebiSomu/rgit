use std::collections::{BTreeMap, HashMap};
use crate::helpers::commit::{current_signature, write_commit_object};
use crate::helpers::format_patch::parse_commit_object;
use crate::helpers::merge_tree::merge_trees_in_memory;
use crate::helpers::objects::flatten_tree;
use crate::helpers::rev_list::{list_all_refs, parse_revision_args, select_rev_list_commits};
use crate::helpers::rev_parse::{peel_to_type, resolve_rev};
use crate::helpers::state_files::build_tree_from_map;
use crate::objects::ReplayOptions;

/// git's usage text for `replay`, printed for `-h` and after usage errors (exit 129).
pub const REPLAY_USAGE: &str = "usage: rgit replay ([--contained] --onto <newbase> | --advance <branch>) <revision-range>...

    --advance <branch>    make replay advance given branch
    --onto <revision>     replay onto given commit
    --contained           advance all branches contained in revision-range
";

/// Parses `replay` arguments like git: `--onto <newbase>` or `--advance <branch>` (exactly
/// one of them), `--contained` (only with `--onto`), and at least one revision range.
/// Errors carry the usage text and mean exit 129.
// Used by `replay`.
pub fn parse_replay_args(args: &[String]) -> anyhow::Result<ReplayOptions> {
    let mut opts = ReplayOptions::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        let mut value = |name: &str| -> anyhow::Result<String> {
            let v = args.get(i).cloned().ok_or_else(|| anyhow::anyhow!("error: option `{}' requires a value\n", name))?;
            i += 1;
            Ok(v)
        };
        match arg {
            "--onto" => opts.onto = Some(value("onto")?),
            "--advance" => opts.advance = Some(value("advance")?),
            "--contained" => opts.contained = true,
            _ if arg.starts_with("--onto=") => opts.onto = Some(arg["--onto=".len()..].to_string()),
            _ if arg.starts_with("--advance=") => opts.advance = Some(arg["--advance=".len()..].to_string()),
            _ if arg.starts_with("--") && arg != "--not" => anyhow::bail!("error: unknown option `{}'\n{}", &arg[2..], REPLAY_USAGE),
            _ => opts.ranges.push(arg.to_string()),
        }
    }
    if opts.onto.is_none() && opts.advance.is_none() {
        anyhow::bail!("error: option --onto or --advance is mandatory\n{}", REPLAY_USAGE);
    }
    if opts.onto.is_some() && opts.advance.is_some() {
        anyhow::bail!("fatal: options '--onto' and '--advance' cannot be used together\n");
    }
    if opts.advance.is_some() && opts.contained {
        anyhow::bail!("fatal: options '--advance' and '--contained' cannot be used together\n");
    }
    if opts.ranges.is_empty() {
        anyhow::bail!("{}", REPLAY_USAGE);
    }
    Ok(opts)
}

/// Port of git's `replay`: re-creates the non-merge commits of the ranges, oldest first,
/// on top of the new base, entirely in the object store. Each commit is redone as a
/// three-way merge (its parent's tree as base, the replayed parent as ours, the commit as
/// theirs), keeping its author and message; the committer is the current user. Returns
/// the `update <ref> <new> <old>` lines for `update-ref --stdin`, or `None` when a commit
/// doesn't apply cleanly. With `--onto` the lines cover the branches named as range tips
/// (every branch pointing into the range with `contained`); with `--advance` the one
/// branch being advanced.
// Used by `replay`.
pub fn replay_commits(opts: &ReplayOptions) -> anyhow::Result<Option<Vec<String>>> {
    let commit_of = |name: &str| resolve_rev(name).and_then(|hash| peel_to_type(hash, "commit")).map_err(|_| anyhow::anyhow!("fatal: '{}' is not a valid commit-ish", name));
    let (onto, advance_ref) = match (&opts.onto, &opts.advance) {
        (Some(onto), _) => (commit_of(onto)?, None),
        (None, Some(branch)) => {
            let full = if branch.starts_with("refs/heads/") { branch.clone() } else { format!("refs/heads/{}", branch) };
            let Some(tip) = list_all_refs().get(&full).cloned() else { anyhow::bail!("fatal: argument to --advance must be a reference") };
            (tip, Some(full))
        }
        (None, None) => unreachable!("checked while parsing"),
    };

    let mut revs = parse_revision_args(&opts.ranges)?;
    revs.topo_order = true;
    let (mut selected, _) = select_rev_list_commits(&revs)?;
    selected.reverse();

    // The branches whose tips the ranges name get moved (`A..topic` names `topic`).
    let refs = list_all_refs();
    let mut tip_refs: Vec<String> = Vec::new();
    for range in &opts.ranges {
        let tip = range.rsplit_once("..").map(|(_, tip)| tip.trim_start_matches('.')).unwrap_or(range);
        if tip.starts_with('^') || tip.is_empty() {
            continue;
        }
        let full = if tip.starts_with("refs/") { tip.to_string() } else { format!("refs/heads/{}", tip) };
        if refs.contains_key(&full) {
            tip_refs.push(full);
        }
    }

    let tree_of = |commit: &str| -> anyhow::Result<BTreeMap<String, ([u8; 20], u32)>> {
        let mut tree = BTreeMap::new();
        flatten_tree(&parse_commit_object(commit)?.tree, "", &mut tree)?;
        Ok(tree)
    };
    let committer = current_signature()?;
    let mut replayed: HashMap<String, String> = HashMap::new();
    let mut updates = Vec::new();
    let mut last = onto.clone();
    for walked in &selected {
        let commit = &walked.commit;
        if commit.parents.len() > 1 {
            anyhow::bail!("fatal: replaying merge commits is not supported yet!");
        }
        let base = match commit.parents.first() {
            Some(parent) => tree_of(parent)?,
            None => BTreeMap::new(),
        };
        let new_parent = commit.parents.first().and_then(|parent| replayed.get(parent)).cloned().unwrap_or_else(|| if advance_ref.is_some() { last.clone() } else { onto.clone() });
        let merged = merge_trees_in_memory(&base, &tree_of(&new_parent)?, &tree_of(&commit.hash)?, "HEAD", &commit.hash[..7], 7)?;
        if !merged.conflicted.is_empty() {
            return Ok(None);
        }
        let tree = if merged.tree.is_empty() { crate::helpers::objects::write_object("tree", b"")? } else { build_tree_from_map(&merged.tree)? };
        let new_commit = write_commit_object(&tree, &[new_parent], &commit.author, &committer, &commit.message)?;
        replayed.insert(commit.hash.clone(), new_commit.clone());
        last = new_commit.clone();

        if advance_ref.is_none() {
            for (name, target) in refs.iter().filter(|(name, target)| name.starts_with("refs/heads/") && **target == commit.hash) {
                if opts.contained || tip_refs.contains(name) {
                    updates.push(format!("update {} {} {}", name, new_commit, target));
                }
            }
        }
    }
    if let Some(branch) = advance_ref {
        updates.push(format!("update {} {} {}", branch, last, onto));
    }
    Ok(Some(updates))
}
