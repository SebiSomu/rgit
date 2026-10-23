use std::path::{Path, PathBuf};
use crate::helpers::config::{
    get_config_value, get_from, global_config_path, read_entries_from, set_in, unset_in,
    LOCAL_CONFIG_PATH,
};

pub fn config(key: Option<String>, value: Option<String>, global: bool, list: bool, unset: bool) -> anyhow::Result<()> {
    let global_path = global_config_path();
    let target: PathBuf = if global {
        global_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("fatal: neither $HOME nor %USERPROFILE% is set; cannot locate the global config"))?
    } else {
        crate::repo::path(LOCAL_CONFIG_PATH)
    };

    if list {
        if global {
            print_entries(&target)?;
        } else {
            if let Some(g) = &global_path {
                print_entries(g)?;
            }
            if crate::repo::exists() {
                print_entries(&crate::repo::path(LOCAL_CONFIG_PATH))?;
            }
        }
        return Ok(());
    }

    let key = key.ok_or_else(|| {
        anyhow::anyhow!("usage: rgit config [--global] <key> [<value>] | --unset <key> | --list")
    })?;

    if unset {
        require_repo_unless_global(global)?;
        if !unset_in(&target, &key)? {
            anyhow::bail!("error: no such key '{}' to unset", key);
        }
        return Ok(());
    }

    match value {
        Some(v) => {
            require_repo_unless_global(global)?;
            set_in(&target, &key, &v)?;
        }
        None => {
            let found = if global {
                get_from(&target, &key)?
            } else {
                get_config_value(&key)?
            };
            match found {
                Some(v) => println!("{}", v),
                None => std::process::exit(1),
            }
        }
    }

    Ok(())
}

fn require_repo_unless_global(global: bool) -> anyhow::Result<()> {
    if !global && !crate::repo::exists() {
        anyhow::bail!("fatal: not in an rgit repository (no .git directory); use --global to write the global config");
    }
    Ok(())
}

fn print_entries(path: &Path) -> anyhow::Result<()> {
    for (k, v) in read_entries_from(path)? {
        println!("{}={}", k, v);
    }
    Ok(())
}