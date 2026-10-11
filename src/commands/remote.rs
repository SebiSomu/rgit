use anyhow::Result;
use std::fs;
use std::path::Path;
use crate::helpers::config::{read_entries_from, write_entries_to, LOCAL_CONFIG_PATH};
use crate::helpers::remote::*;
use crate::objects::RemoteAction;

/// `rgit remote [-v | --verbose]`
/// `rgit remote add <name> <url>`
/// `rgit remote remove <name>` / `rm <name>`
/// `rgit remote rename <old> <new>`
/// `rgit remote get-url [--push] <name>`
/// `rgit remote set-url <name> <newurl>`
/// `rgit remote show <name>`
///
/// Manages the set of tracked repositories (remotes) in `.git/config`.
/// Handles listing with verbose URLs, adding remotes with default fetch refspecs,
/// deleting/renaming remote references and tracking branches, and querying remote metadata.
// Used by CLI command dispatch (`cli.rs`).
pub fn remote(verbose: bool, action: Option<RemoteAction>) -> Result<()> {
    match action {
        None => list_remotes(verbose),
        Some(RemoteAction::Add { name, url }) => add_remote_cmd(&name, &url),
        Some(RemoteAction::Remove { name }) | Some(RemoteAction::Rm { name }) => remove_remote_cmd(&name),
        Some(RemoteAction::Rename { old_name, new_name }) => rename_remote_cmd(&old_name, &new_name),
        Some(RemoteAction::GetUrl { push, name }) => get_url_cmd(&name, push),
        Some(RemoteAction::SetUrl { name, new_url }) => set_url_cmd(&name, &new_url),
        Some(RemoteAction::Show { name }) => show_remote_cmd(&name),
    }
}

/// Lists all configured remote names, or verbose URLs if `verbose` is true.
fn list_remotes(verbose: bool) -> Result<()> {
    let remotes = list_remote_names()?;

    for name in &remotes {
        if verbose {
            let (fetch_url, push_url) = get_remote_urls(name)?;
            if let Some(ref url) = fetch_url {
                println!("{}\t{} (fetch)", name, url);
            }
            if let Some(ref url) = push_url {
                println!("{}\t{} (push)", name, url);
            }
        } else {
            println!("{}", name);
        }
    }

    Ok(())
}

/// Adds a new remote entry to the repository config.
fn add_remote_cmd(name: &str, url: &str) -> Result<()> {
    add_remote(name, url)?;
    Ok(())
}

/// Removes a remote, its configuration, and associated remote-tracking branches.
fn remove_remote_cmd(name: &str) -> Result<()> {
    remove_remote(name)?;
    Ok(())
}

/// Renames an existing remote, its ref directories, and any tracking branch associations.
fn rename_remote_cmd(old_name: &str, new_name: &str) -> Result<()> {
    rename_remote(old_name, new_name)?;
    Ok(())
}

/// Retrieves the fetch or push URL for a specified remote.
fn get_url_cmd(name: &str, push: bool) -> Result<()> {
    if !remote_exists(name)? {
        anyhow::bail!("fatal: No such remote '{}'", name);
    }

    let (fetch_url, push_url) = get_remote_urls(name)?;
    let url = if push {
        push_url.or(fetch_url)
    } else {
        fetch_url
    };

    match url {
        Some(u) => {
            println!("{}", u);
            Ok(())
        }
        None => anyhow::bail!("fatal: No URL found for remote '{}'", name),
    }
}

/// Updates the URL for an existing remote in `.git/config`.
fn set_url_cmd(name: &str, new_url: &str) -> Result<()> {
    if !remote_exists(name)? {
        anyhow::bail!("fatal: No such remote '{}'", name);
    }

    let config_path = Path::new(LOCAL_CONFIG_PATH);
    let mut entries = read_entries_from(config_path)?;
    let url_key = format!("remote.{}.url", name);

    let mut found = false;
    for (k, v) in entries.iter_mut() {
        if k == &url_key {
            *v = new_url.to_string();
            found = true;
            break;
        }
    }

    if !found {
        entries.push((url_key, new_url.to_string()));
    }

    write_entries_to(config_path, &entries)?;
    Ok(())
}

/// Shows detailed information about a remote repository.
fn show_remote_cmd(name: &str) -> Result<()> {
    let info = get_remote_info(name)?;

    println!("* remote {}", info.name);
    println!("  Fetch URL: {}", info.fetch_url.as_deref().unwrap_or("(none)"));
    println!("  Push  URL: {}", info.push_url.as_deref().unwrap_or("(none)"));

    let head_path = format!(".git/refs/remotes/{}/HEAD", name);
    let head_branch = if Path::new(&head_path).exists() {
        fs::read_to_string(&head_path)
            .ok()
            .map(|s| s.trim().strip_prefix("ref: refs/remotes/").unwrap_or(s.trim()).to_string())
    } else {
        None
    };

    if let Some(hb) = head_branch {
        println!("  HEAD branch: {}", hb);
    } else {
        println!("  HEAD branch: (unknown)");
    }

    if info.tracking_branches.is_empty() {
        println!("  Remote branch(es): (none)");
    } else {
        println!("  Remote branch(es):");
        for b in &info.tracking_branches {
            println!("    {} tracked", b);
        }
    }

    Ok(())
}
