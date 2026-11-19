use std::path::Path;
use anyhow::Result;

use crate::helpers::maintenance::{
    config_path_values, run_maintenance_task, run_rgit_in, selected_maintenance_tasks, update_maintenance_registration, FOR_EACH_REPO_USAGE, MAINTENANCE_TASKS, MAINTENANCE_USAGE,
};

/// `rgit for-each-repo --config=<config> [--keep-going] [--] <arguments>`
///
/// Runs `rgit <arguments>` in every repository listed under the multi-valued config key
/// `<config>` (for example `maintenance.repo`), one after another. It stops at the first
/// repository where the command fails and exits with that status; with `--keep-going` it
/// carries on and exits 1 at the end if any failed. A key without values does nothing.
/// Exit 129 on usage errors.
pub fn for_each_repo(args: Vec<String>) -> Result<()> {
    let usage = |message: &str| -> ! {
        if !message.is_empty() {
            eprintln!("{}", message);
        }
        eprint!("{}", FOR_EACH_REPO_USAGE);
        crate::ui::exit(129);
    };
    let (mut key, mut keep_going) = (None, false);
    let mut command: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        match arg {
            "-h" => usage(""),
            "--" => {
                command = args[i..].to_vec();
                break;
            }
            "--keep-going" => keep_going = true,
            "--config" => {
                key = Some(args.get(i).cloned().unwrap_or_else(|| usage("error: option `config' requires a value")));
                i += 1;
            }
            _ if arg.starts_with("--config=") => key = Some(arg["--config=".len()..].to_string()),
            _ if arg.starts_with('-') => usage(&format!("error: unknown option `{}'", arg.trim_start_matches('-'))),
            _ => {
                command = args[i - 1..].to_vec();
                break;
            }
        }
    }
    let Some(key) = key else {
        eprintln!("fatal: missing --config=<config>");
        crate::ui::exit(128);
    };

    let mut failed = false;
    for repo in config_path_values(&key) {
        let status = run_rgit_in(Path::new(&repo), &command);
        if status != 0 {
            if !keep_going {
                crate::ui::exit(status);
            }
            failed = true;
        }
    }
    if failed {
        crate::ui::exit(1);
    }
    Ok(())
}

/// `rgit maintenance run [--auto] [--quiet] [--task=<task>]...` / `register` /
/// `unregister [--force]`
///
/// Runs repository upkeep. `run` performs the tasks named with `--task` (in that order),
/// or those enabled through `maintenance.<task>.enabled`, or just `gc` by default. `gc`
/// runs `rgit gc` and `pack-refs` runs `rgit pack-refs --all --prune`; `commit-graph`,
/// `loose-objects`, `incremental-repack` and `prefetch` are accepted but skipped with a
/// warning, because rgit has no packfiles or commit-graph to maintain. `--auto` runs `gc`
/// only once the repository holds more loose objects than `gc.auto` (6700). `register`
/// adds this repository to the global `maintenance.repo` list (so `for-each-repo
/// --config=maintenance.repo maintenance run` covers it) and `unregister` removes it
/// (`--force` makes that a no-op when it isn't listed). `start` and `stop` are not
/// supported: rgit installs no background scheduler. Exit status: 0 success, 1 a failed
/// task, 128 fatal, 129 usage.
pub fn maintenance(args: Vec<String>) -> Result<()> {
    let usage = |message: &str| -> ! {
        if !message.is_empty() {
            eprintln!("{}", message);
        }
        eprint!("{}", MAINTENANCE_USAGE);
        crate::ui::exit(129);
    };
    let fatal = |message: String| -> ! {
        eprintln!("{}", message);
        crate::ui::exit(128);
    };
    let Some((subcommand, rest)) = args.split_first() else { usage("") };
    if subcommand == "-h" || rest.iter().any(|a| a == "-h") {
        usage("");
    }
    if !crate::core::repo::exists() {
        fatal("fatal: not a git repository (or any of the parent directories): .git".to_string());
    }

    match subcommand.as_str() {
        "run" => {
            let (mut auto, mut quiet) = (false, false);
            let mut tasks: Vec<String> = Vec::new();
            let mut i = 0;
            while i < rest.len() {
                let arg = rest[i].as_str();
                i += 1;
                let task = match arg {
                    "--auto" => {
                        auto = true;
                        continue;
                    }
                    "--quiet" => {
                        quiet = true;
                        continue;
                    }
                    "--no-quiet" => {
                        quiet = false;
                        continue;
                    }
                    "--task" => {
                        i += 1;
                        rest.get(i - 1).cloned().unwrap_or_else(|| usage("error: option `task' requires a value"))
                    }
                    _ if arg.starts_with("--task=") => arg["--task=".len()..].to_string(),
                    _ => usage(&format!("error: unknown option `{}'", arg.trim_start_matches('-'))),
                };
                if !MAINTENANCE_TASKS.contains(&task.as_str()) {
                    usage(&format!("error: '{}' is not a valid task", task));
                }
                if tasks.contains(&task) {
                    usage(&format!("error: task '{}' cannot be selected multiple times", task));
                }
                tasks.push(task);
            }
            let mut failed = false;
            for task in selected_maintenance_tasks(&tasks) {
                if !run_maintenance_task(&task, auto, quiet) {
                    eprintln!("error: task '{}' failed", task);
                    failed = true;
                }
            }
            if failed {
                crate::ui::exit(1);
            }
        }
        "register" | "unregister" => {
            let force = rest.iter().any(|a| a == "--force");
            if rest.iter().any(|a| a != "--force") || (force && subcommand == "register") {
                usage("");
            }
            let repo = std::env::current_dir().map(|dir| dir.display().to_string().replace('\\', "/")).unwrap_or_else(|e| fatal(format!("fatal: {}", e)));
            match update_maintenance_registration(&repo, subcommand == "register") {
                Ok(true) => {}
                Ok(false) if force => {}
                Ok(false) => fatal(format!("fatal: repository '{}' is not registered", repo)),
                Err(e) => fatal(e.to_string()),
            }
        }
        "start" | "stop" => fatal(format!("fatal: 'maintenance {}' is not supported by rgit: it installs no background scheduler", subcommand)),
        other => usage(&format!("error: unknown subcommand: `{}'", other)),
    }
    Ok(())
}
