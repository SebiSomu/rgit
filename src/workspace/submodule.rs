pub mod engine;
pub mod types;

use std::fs;
use std::path::Path;
use std::process::Command;
use anyhow::Result;

use crate::core::config::{set_in, unset_in, read_entries_from, write_entries_to, LOCAL_CONFIG_PATH};
use crate::workspace::index_plumbing::bare_index_entry;
use crate::workspace::submodule::engine::{describe_submodule_commit, read_gitmodules, selected_submodules, stage_gitmodules, registered_url, resolve_submodule_url, run_rgit_in, submodule_git_dir, submodule_head, SUBMODULE_USAGE};
use crate::core::index::{read_index, write_index};
use crate::core::repo;

/// Prints a message and exits.
fn die(message: &str, code: i32) -> ! {
    eprint!("{}", message);
    crate::ui::exit(code);
}

/// `rgit submodule [add | status | init | update | deinit | foreach | sync | set-url | set-branch]`
///
/// Keeps other repositories inside this one. A submodule is recorded in two places: a
/// *gitlink* entry (mode 160000) in the index and trees, naming the commit of the other
/// repository that belongs to this state of the project, and a `.gitmodules` file with its
/// path and URL.
/// - `add [-b <branch>] [--name <name>] <repository> [<path>]` clones the repository into
///   `<path>` and stages the gitlink and `.gitmodules`.
/// - `status [--cached] [--recursive]` prints one line per submodule: its commit, path and a
///   name for the commit; a leading `-` means not initialized, `+` that the checked-out
///   commit differs from the staged one.
/// - `init` copies the URLs from `.gitmodules` into `.git/config`; `update [--init]
///   [--recursive]` clones missing submodules and checks out, detached, the commit the
///   superproject records.
/// - `deinit [-f] (--all | <path>...)` unregisters submodules and empties their directories.
/// - `foreach [--recursive] <command>` runs a shell command in each checked-out submodule
///   (`$name`, `$sm_path`, `$displaypath`, `$sha1` and `$toplevel` are set).
/// - `sync` re-copies the URLs after `.gitmodules` changed; `set-url <path> <url>` and
///   `set-branch (-b <branch> | -d) <path>` edit `.gitmodules`.
/// Elsewhere in rgit: `add <path>` stages a submodule's current commit, `status` reports
/// `(new commits)`, `checkout`/`switch`/`reset`/`clone` create an empty directory for a
/// gitlink, and diffs show `Subproject commit <hash>`. Unlike git, a submodule's repository
/// lives in `<path>/.git` itself rather than under `.git/modules/` (git reads both
/// layouts); `deinit` moves it to `.git/modules/<name>` so nothing is lost, and `update`
/// brings it back. Not available: `summary`, `absorbgitdirs`, `--remote`, `--merge`,
/// `--rebase`. Exit status: 0, 1 on failures, 128 on fatal errors, 129 for a usage error.
pub fn submodule(args: Vec<String>) -> Result<()> {
    let mut args = args;
    let mut quiet = false;
    while args.first().is_some_and(|a| a == "--quiet" || a == "-q") {
        quiet = true;
        args.remove(0);
    }
    if !repo::exists() {
        die("fatal: not a git repository (or any of the parent directories): .git\n", 128);
    }
    let action = match args.first().map(String::as_str) {
        None => "status".to_string(),
        Some("-h") => {
            print!("{}", SUBMODULE_USAGE);
            crate::ui::exit(129);
        }
        Some(flag) if flag.starts_with('-') => "status".to_string(),
        Some(name) => {
            let name = name.to_string();
            args.remove(0);
            name
        }
    };

    // Shared option parsing: switches, value options and path arguments.
    let (mut cached, mut recursive, mut init, mut force, mut all, mut default_branch) = (false, false, false, false, false, false);
    let (mut branch, mut name): (Option<String>, Option<String>) = (None, None);
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].clone();
        i += 1;
        if action == "foreach" && !arg.starts_with("--") {
            rest.push(arg);
            rest.extend(args[i..].iter().cloned());
            break;
        }
        match arg.as_str() {
            "--cached" => cached = true,
            "--recursive" => recursive = true,
            "--init" => init = true,
            "-f" | "--force" => force = true,
            "--all" => all = true,
            "-d" | "--default" => default_branch = true,
            "-q" | "--quiet" => quiet = true,
            "--progress" | "--no-fetch" | "--checkout" | "--recommend-shallow" | "--no-recommend-shallow" => {}
            "-b" | "--branch" => {
                branch = args.get(i).cloned();
                i += 1;
            }
            "--name" => {
                name = args.get(i).cloned();
                i += 1;
            }
            "--" => {
                rest.extend(args[i..].iter().cloned());
                break;
            }
            other if other.starts_with("--branch=") => branch = Some(other["--branch=".len()..].to_string()),
            other if other.starts_with("--name=") => name = Some(other["--name=".len()..].to_string()),
            "--remote" | "--merge" | "--rebase" | "--depth" | "--reference" | "--jobs" | "--single-branch" => die(&format!("fatal: {} is not supported by rgit submodule\n", arg), 128),
            other if other.starts_with('-') => die(&format!("error: unknown option `{}'\n{}", other.trim_start_matches('-'), SUBMODULE_USAGE), 129),
            _ => rest.push(arg),
        }
    }
    let config = repo::path(LOCAL_CONFIG_PATH);
    let toplevel = std::env::current_dir()?;
    let say = |text: String| {
        if !quiet {
            println!("{}", text);
        }
    };

    match action.as_str() {
        "add" => {
            let Some(url) = rest.first().cloned() else { die(SUBMODULE_USAGE, 129) };
            let path = match rest.get(1) {
                Some(path) => path.trim_end_matches('/').to_string(),
                None => url.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or("").to_string(),
            };
            if path.is_empty() {
                die(&format!("fatal: cannot guess a directory name from '{}'\n", url), 128);
            }
            let name = name.unwrap_or_else(|| path.clone());
            let mut entries = read_index().unwrap_or_default();
            if entries.iter().any(|e| e.path == path || e.path.starts_with(&format!("{}/", path))) {
                die(&format!("fatal: '{}' already exists in the index\n", path), 128);
            }
            if submodule_git_dir(&path).is_none() {
                if Path::new(&path).exists() && fs::read_dir(&path).map(|mut d| d.next().is_some()).unwrap_or(true) {
                    die(&format!("fatal: '{}' already exists and is not a valid git repo\n", path), 128);
                }
                let source = resolve_submodule_url(&url);
                if !run_rgit_in(&toplevel, &["clone", &source, &path], quiet)? {
                    die(&format!("fatal: clone of '{}' into submodule path '{}' failed\n", url, path), 128);
                }
                if let Some(branch) = &branch {
                    if !run_rgit_in(Path::new(&path), &["switch", branch], quiet)? {
                        die(&format!("fatal: unable to check out branch '{}' in submodule path '{}'\n", branch, path), 128);
                    }
                }
            } else {
                say(format!("Adding existing repo at '{}' to the index", path));
            }
            let Some(head) = submodule_head(&path) else { die(&format!("fatal: no commit checked out in submodule path '{}'\n", path), 128) };
            let gitmodules = Path::new(".gitmodules");
            set_in(gitmodules, &format!("submodule.{}.path", name), &path)?;
            set_in(gitmodules, &format!("submodule.{}.url", name), &url)?;
            if let Some(branch) = &branch {
                set_in(gitmodules, &format!("submodule.{}.branch", name), branch)?;
            }
            set_in(&config, &format!("submodule.{}.url", name), &resolve_submodule_url(&url))?;
            set_in(&config, &format!("submodule.{}.active", name), "true")?;
            let mut hash = [0u8; 20];
            hex::decode_to_slice(&head, &mut hash)?;
            entries.push(bare_index_entry(&path, hash, 0o160000));
            write_index(&mut entries)?;
            stage_gitmodules()?;
        }
        "status" => {
            for module in selected_submodules(&rest)? {
                let Some(staged) = &module.staged else { continue };
                let head = submodule_head(&module.path);
                match head {
                    None => println!("-{} {}", staged, module.path),
                    Some(head) => {
                        let shown = if cached { staged.clone() } else { head.clone() };
                        let mark = if head == *staged { ' ' } else { '+' };
                        println!("{}{} {} ({})", mark, shown, module.path, describe_submodule_commit(&module.path, &shown));
                        if recursive && Path::new(&module.path).join(".gitmodules").exists() {
                            let output = Command::new(std::env::current_exe()?).current_dir(&module.path).args(["submodule", "status", "--recursive"]).output()?;
                            for line in String::from_utf8_lossy(&output.stdout).lines() {
                                // Nested paths are shown relative to this repository.
                                match line.char_indices().nth(42).map(|(at, _)| line.split_at(at)) {
                                    Some((head, tail)) => println!("{}{}/{}", head, module.path, tail),
                                    None => println!("{}", line),
                                }
                            }
                        }
                    }
                }
            }
        }
        "init" => {
            for module in selected_submodules(&rest)? {
                if module.staged.is_none() || registered_url(&module.name).is_some() {
                    continue;
                }
                if module.url.is_empty() {
                    die(&format!("fatal: No url found for submodule path '{}' in .gitmodules\n", module.path), 128);
                }
                let url = resolve_submodule_url(&module.url);
                set_in(&config, &format!("submodule.{}.active", module.name), "true")?;
                set_in(&config, &format!("submodule.{}.url", module.name), &url)?;
                say(format!("Submodule '{}' ({}) registered for path '{}'", module.name, url, module.path));
            }
        }
        "update" => {
            let mut failed = false;
            for module in selected_submodules(&rest)? {
                let Some(staged) = &module.staged else { continue };
                let mut url = registered_url(&module.name);
                if url.is_none() {
                    if !init {
                        continue;
                    }
                    if module.url.is_empty() {
                        die(&format!("fatal: No url found for submodule path '{}' in .gitmodules\n", module.path), 128);
                    }
                    let resolved = resolve_submodule_url(&module.url);
                    set_in(&config, &format!("submodule.{}.active", module.name), "true")?;
                    set_in(&config, &format!("submodule.{}.url", module.name), &resolved)?;
                    say(format!("Submodule '{}' ({}) registered for path '{}'", module.name, resolved, module.path));
                    url = Some(resolved);
                }
                let url = url.unwrap_or_default();
                let path = Path::new(&module.path);
                let mut fresh = false;
                if submodule_git_dir(&module.path).is_none() {
                    fresh = true;
                    // A repository set aside by `deinit` is brought back; otherwise clone.
                    let parked = repo::path(format!(".git/modules/{}", module.name));
                    if parked.join("HEAD").exists() {
                        fs::create_dir_all(path)?;
                        fs::rename(&parked, path.join(".git"))?;
                        run_rgit_in(path, &["reset", "--hard"], true)?;
                    } else if !run_rgit_in(&toplevel, &["clone", &url, &module.path], true)? {
                        eprintln!("fatal: clone of '{}' into submodule path '{}' failed", url, toplevel.join(path).display());
                        failed = true;
                        continue;
                    }
                }
                if submodule_head(&module.path).as_deref() != Some(staged.as_str()) {
                    // The commit may be newer than what the submodule has: fetch and retry.
                    let checkout = |quiet_run: bool| run_rgit_in(path, &["checkout", staged], quiet_run);
                    if !checkout(true)? {
                        run_rgit_in(path, &["fetch"], true)?;
                        if !checkout(true)? {
                            eprintln!("fatal: Unable to checkout '{}' in submodule path '{}'", staged, module.path);
                            failed = true;
                            continue;
                        }
                    }
                    say(format!("Submodule path '{}': checked out '{}'", module.path, staged));
                } else if fresh {
                    say(format!("Submodule path '{}': checked out '{}'", module.path, staged));
                }
                if recursive && path.join(".gitmodules").exists() && !run_rgit_in(path, &["submodule", "update", "--init", "--recursive"], quiet)? {
                    failed = true;
                }
            }
            if failed {
                crate::ui::exit(1);
            }
        }
        "deinit" => {
            if rest.is_empty() && !all {
                die("fatal: Use '--all' if you really want to deinitialize all submodules\n", 128);
            }
            for module in selected_submodules(&rest)? {
                let path = Path::new(&module.path);
                if let Some(git_dir) = submodule_git_dir(&module.path) {
                    if !force && submodule_head(&module.path) != module.staged {
                        die(&format!("error: Submodule work tree '{}' contains local modifications; use '-f' to discard them\n", module.path), 1);
                    }
                    // Keep the repository itself: park it under .git/modules/<name>.
                    if git_dir == path.join(".git") {
                        let parked = repo::path(format!(".git/modules/{}", module.name));
                        if let Some(parent) = parked.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        if parked.exists() {
                            fs::remove_dir_all(&parked)?;
                        }
                        fs::rename(&git_dir, &parked)?;
                    }
                    fs::remove_dir_all(path)?;
                    fs::create_dir_all(path)?;
                    say(format!("Cleared directory '{}'", module.path));
                }
                if let Some(url) = registered_url(&module.name) {
                    unset_in(&config, &format!("submodule.{}.url", module.name))?;
                    let _ = unset_in(&config, &format!("submodule.{}.active", module.name));
                    say(format!("Submodule '{}' ({}) unregistered for path '{}'", module.name, if module.url.is_empty() { url } else { module.url.clone() }, module.path));
                }
            }
        }
        "foreach" => {
            if rest.is_empty() {
                die(SUBMODULE_USAGE, 129);
            }
            let script = rest.join(" ");
            for module in read_gitmodules()? {
                let (Some(staged), Some(_)) = (&module.staged, submodule_git_dir(&module.path)) else { continue };
                say(format!("Entering '{}'", module.path));
                let status = Command::new("sh")
                    .arg("-c")
                    .arg(&script)
                    .current_dir(&module.path)
                    .env("name", &module.name)
                    .env("sm_path", &module.path)
                    .env("displaypath", &module.path)
                    .env("sha1", staged)
                    .env("toplevel", &toplevel)
                    .status()?;
                if !status.success() {
                    die(&format!("fatal: run_command returned non-zero status for {}\n.\n", module.path), 128);
                }
                if recursive && Path::new(&module.path).join(".gitmodules").exists() {
                    let mut nested = vec!["submodule", "foreach", "--recursive"];
                    nested.push(&script);
                    if !run_rgit_in(Path::new(&module.path), &nested, false)? {
                        die(&format!("fatal: run_command returned non-zero status while recursing in the nested submodules of {}\n.\n", module.path), 128);
                    }
                }
            }
        }
        "sync" => {
            for module in selected_submodules(&rest)? {
                if registered_url(&module.name).is_none() {
                    continue;
                }
                let url = resolve_submodule_url(&module.url);
                say(format!("Synchronizing submodule url for '{}'", module.path));
                set_in(&config, &format!("submodule.{}.url", module.name), &url)?;
                if let Some(git_dir) = submodule_git_dir(&module.path) {
                    set_in(&git_dir.join("config"), "remote.origin.url", &url)?;
                }
            }
        }
        "set-url" => {
            let [path, url] = rest.as_slice() else { die(SUBMODULE_USAGE, 129) };
            let module = selected_submodules(std::slice::from_ref(path))?.remove(0);
            set_in(Path::new(".gitmodules"), &format!("submodule.{}.url", module.name), url)?;
            let resolved = resolve_submodule_url(url);
            if registered_url(&module.name).is_some() {
                set_in(&config, &format!("submodule.{}.url", module.name), &resolved)?;
            }
            if let Some(git_dir) = submodule_git_dir(&module.path) {
                set_in(&git_dir.join("config"), "remote.origin.url", &resolved)?;
            }
            say(format!("Synchronizing submodule url for '{}'", module.path));
        }
        "set-branch" => {
            let [path] = rest.as_slice() else { die(SUBMODULE_USAGE, 129) };
            let module = selected_submodules(std::slice::from_ref(path))?.remove(0);
            let key = format!("submodule.{}.branch", module.name);
            match (&branch, default_branch) {
                (Some(branch), false) => set_in(Path::new(".gitmodules"), &key, branch)?,
                (None, true) => {
                    // Drop the key but keep the rest of the file as it is.
                    let file = Path::new(".gitmodules");
                    let kept: Vec<(String, String)> = read_entries_from(file)?.into_iter().filter(|(k, _)| *k != key).collect();
                    write_entries_to(file, &kept)?;
                }
                _ => die("fatal: options '--branch' and '--default' cannot be used together (one is required)\n", 128),
            }
        }
        "summary" | "absorbgitdirs" => die(&format!("fatal: submodule {} is not supported by rgit\n", action), 128),
        other => die(&format!("error: unknown subcommand: `{}'\n{}", other, SUBMODULE_USAGE), 129),
    }
    Ok(())
}
