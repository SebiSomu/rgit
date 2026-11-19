use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use crate::core::config::{get_config_value, global_config_path, read_all_config_entries, read_entries_from, write_entries_to};

/// Usage text for `for-each-repo`, printed for `-h` and after usage errors (exit 129).
pub const FOR_EACH_REPO_USAGE: &str = "usage: rgit for-each-repo --config=<config> [--keep-going] [--] <arguments>

    --config <config>     config key storing a list of repository paths
    --keep-going          keep going even if command fails in a repository
";

/// Usage text for `maintenance`, printed for `-h` and after usage errors (exit 129).
pub const MAINTENANCE_USAGE: &str = "usage: rgit maintenance run [--auto] [--[no-]quiet] [--task=<task>]
   or: rgit maintenance register
   or: rgit maintenance unregister [--force]
";

/// The maintenance tasks git knows, in the order they run.
pub const MAINTENANCE_TASKS: [&str; 6] = ["prefetch", "loose-objects", "incremental-repack", "gc", "commit-graph", "pack-refs"];

/// Loose objects above which `maintenance run --auto` considers `gc` due (git's `gc.auto` default).
const DEFAULT_GC_AUTO: usize = 6700;

/// Returns every value of a multi-valued config key (global, then local), with a leading
/// `~/` expanded to the home directory, as git does for path values.
// Used by `for-each-repo` and `maintenance unregister`.
pub fn config_path_values(key: &str) -> Vec<String> {
    let key = key.to_ascii_lowercase();
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    read_all_config_entries()
        .into_iter()
        .filter(|(k, _)| k.to_ascii_lowercase() == key)
        .map(|(_, value)| match value.strip_prefix("~/") {
            Some(rest) if !home.is_empty() => format!("{}/{}", home, rest),
            _ => value,
        })
        .collect()
}

/// Runs this rgit binary with `args` inside the directory `repo` and returns its exit
/// status (1 when it can't be started, for example because the directory is missing).
// Used by `for-each-repo` and `maintenance run`.
pub fn run_rgit_in(repo: &Path, args: &[String]) -> i32 {
    let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rgit"));
    match Command::new(program).args(args).current_dir(repo).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("error: cannot run rgit in '{}': {}", repo.display(), e);
            1
        }
    }
}

/// Counts the loose objects in `.git/objects` (files in the two-character fan-out directories).
// Used by `maintenance run --auto`.
fn count_loose_objects() -> usize {
    let Ok(dirs) = fs::read_dir(crate::core::repo::path(".git/objects")) else { return 0 };
    dirs.flatten().filter(|d| d.file_name().len() == 2).filter_map(|d| fs::read_dir(d.path()).ok()).map(|files| files.count()).sum()
}

/// Decides which tasks `maintenance run` performs: the `--task` ones in the order given,
/// else those enabled by `maintenance.<task>.enabled`, else `gc` alone (git's default).
// Used by `maintenance run`.
pub fn selected_maintenance_tasks(requested: &[String]) -> Vec<String> {
    if !requested.is_empty() {
        return requested.to_vec();
    }
    let enabled = |task: &str| get_config_value(&format!("maintenance.{}.enabled", task)).ok().flatten().map(|v| v.eq_ignore_ascii_case("true"));
    let configured: Vec<String> = MAINTENANCE_TASKS.iter().filter(|task| enabled(task) == Some(true)).map(|task| task.to_string()).collect();
    if !configured.is_empty() || enabled("gc") == Some(false) {
        return configured;
    }
    vec!["gc".to_string()]
}

/// Runs one maintenance task in the current repository and returns whether it succeeded.
/// `gc` runs `rgit gc` (with `auto`, only once there are more loose objects than `gc.auto`,
/// 6700 by default) and `pack-refs` runs `rgit pack-refs --all --prune`. The remaining
/// tasks maintain packfiles, the commit-graph or a prefetch namespace, none of which rgit
/// has, so they are skipped with a note (unless `quiet`) and count as done.
// Used by `maintenance run`.
pub fn run_maintenance_task(task: &str, auto: bool, quiet: bool) -> bool {
    let here = Path::new(".");
    match task {
        "gc" => {
            if auto {
                let limit = get_config_value("gc.auto").ok().flatten().and_then(|v| v.parse::<usize>().ok()).unwrap_or(DEFAULT_GC_AUTO);
                if limit == 0 || count_loose_objects() <= limit {
                    return true;
                }
            }
            run_rgit_in(here, &["gc".to_string()]) == 0
        }
        "pack-refs" => run_rgit_in(here, &["pack-refs".to_string(), "--all".to_string(), "--prune".to_string()]) == 0,
        _ => {
            if !quiet {
                eprintln!("warning: skipping maintenance task '{}': rgit stores objects loose and has nothing for it to do", task);
            }
            true
        }
    }
}

/// Adds (`register`) or removes the repository at `repo` from the global
/// `maintenance.repo` list, like `git maintenance register` / `unregister`. Returns false
/// when asked to remove a repository that isn't listed.
// Used by `maintenance register` and `maintenance unregister`.
pub fn update_maintenance_registration(repo: &str, register: bool) -> anyhow::Result<bool> {
    let Some(global) = global_config_path() else { anyhow::bail!("fatal: could not determine the global configuration file") };
    let mut entries = read_entries_from(&global).unwrap_or_default();
    let listed = entries.iter().any(|(key, value)| key == "maintenance.repo" && value == repo);
    if register {
        if !listed {
            entries.push(("maintenance.repo".to_string(), repo.to_string()));
            write_entries_to(&global, &entries)?;
        }
        return Ok(true);
    }
    if !listed {
        return Ok(false);
    }
    entries.retain(|(key, value)| !(key == "maintenance.repo" && value == repo));
    write_entries_to(&global, &entries)?;
    Ok(true)
}
