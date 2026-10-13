use anyhow::Result;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::cli::ReflogAction;
use crate::helpers::reflog::{
    expire_reflog, format_reflog_output, list_reflog_refs, parse_reflog_spec, read_reflog,
    resolve_reflog_ref_name, write_reflog,
};

/// `rgit reflog [show [<ref>] [-n <count>] | exists <ref> | delete <ref>@{<n>}... |
/// expire [--all] [--expire <days>|now|never] [<ref>]]`
///
/// The reflog records every move of `HEAD`, each branch and each remote-tracking branch, so a
/// commit that is no longer on any branch (after a reset, an amend or a rebase) can still be
/// found and brought back, e.g. `rgit reset --hard HEAD@{1}`. A bare `rgit reflog` is
/// `reflog show HEAD`.
///
/// - `show` lists a ref's entries newest first as `<hash> <ref>@{n}: <message>`.
/// - `exists` produces no output and exits 0 if the ref has a reflog, 1 if not.
/// - `delete` removes individual entries (`HEAD@{2}`), renumbering the ones after them.
/// - `expire` drops entries older than `--expire` days (default 90; `now` drops everything,
///   `never` nothing), for one ref (default HEAD) or all of them with `--all`.
pub fn reflog(action: Option<ReflogAction>) -> Result<()> {
    match action.unwrap_or(ReflogAction::Show { reference: None, max_count: None }) {
        ReflogAction::Show { reference, max_count } => {
            let name = reference.unwrap_or_else(|| "HEAD".to_string());
            let ref_name = resolve_reflog_ref_name(&name);
            let entries = read_reflog(&ref_name)?;
            if entries.is_empty() {
                anyhow::bail!("fatal: no reflog for '{}'", name);
            }
            print!("{}", format_reflog_output(&name, &entries, max_count));
        }

        ReflogAction::Exists { reference } => {
            let ref_name = resolve_reflog_ref_name(&reference);
            if read_reflog(&ref_name)?.is_empty() {
                std::process::exit(1);
            }
        }

        ReflogAction::Delete { entries } => {
            if entries.is_empty() {
                anyhow::bail!("usage: rgit reflog delete <ref>@{{<n>}}...");
            }
            
            let mut wanted: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for spec in &entries {
                let (name, index) = parse_reflog_spec(spec)
                    .ok_or_else(|| anyhow::anyhow!("fatal: '{}' is not of the form <ref>@{{<n>}}", spec))?;
                wanted.entry(resolve_reflog_ref_name(&name)).or_default().push(index);
            }

            for (ref_name, mut indices) in wanted {
                let mut log = read_reflog(&ref_name)?;
                indices.sort_unstable_by(|a, b| b.cmp(a));
                indices.dedup();
                for index in indices {
                    if index >= log.len() {
                        anyhow::bail!("fatal: log for '{}' only has {} entries", ref_name, log.len());
                    }
                    log.remove(index);
                }
                write_reflog(&ref_name, &log)?;
            }
        }

        ReflogAction::Expire { all, expire, reference } => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let cutoff = match expire.as_str() {
                "never" => None,
                "now" => Some(now + 1),
                days => {
                    let days: u64 = days.parse().map_err(|_| {
                        anyhow::anyhow!("fatal: invalid --expire value '{}' (expected a number of days, 'now' or 'never')", days)
                    })?;
                    Some(now.saturating_sub(days.saturating_mul(86_400)))
                }
            };

            let refs = if all {
                list_reflog_refs()?
            } else {
                vec![resolve_reflog_ref_name(&reference.unwrap_or_else(|| "HEAD".to_string()))]
            };

            for ref_name in refs {
                let removed = expire_reflog(&ref_name, cutoff)?;
                if removed > 0 {
                    println!("Expired {} entr{} from {}", removed, if removed == 1 { "y" } else { "ies" }, ref_name);
                }
            }
        }
    }

    Ok(())
}