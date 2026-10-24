use std::collections::HashSet;
use std::fs;
use std::io::{self, IsTerminal, Write};
use anyhow::Result;

use crate::helpers::bundle::{
    enumerate_bundle_objects, format_bundle_header, missing_bundle_prerequisites, parse_bundle_header, read_packfile, resolve_bundle_revs,
    select_bundle_commits, store_loose_objects, write_packfile,
};
use crate::objects::{BundleAction, BundleHeader};
use crate::repo;

/// `rgit bundle create [-q] [--version=<2|3>] <file> <rev-list-args>...`
/// `rgit bundle verify [-q] <file>`
/// `rgit bundle list-heads <file> [<refname>...]`
/// `rgit bundle unbundle <file> [<refname>...]`
///
/// Bundles move history without a network connection: `create` writes one file holding a
/// header (the refs it carries and the prerequisite commits a receiver must already have)
/// followed by a packfile of every object needed beyond those prerequisites. `verify` checks
/// that this repository has the prerequisites and describes the bundle, `list-heads` prints
/// its refs, and `unbundle` stores its objects here (as loose objects) and prints its refs,
/// without updating any ref. The file format is git's, so bundles move freely between rgit
/// and git. Exits 128 on errors, and 1 when a bundle can't be read or its prerequisites are missing.
pub fn bundle(action: BundleAction) -> Result<()> {
    let needs_repo = !matches!(action, BundleAction::ListHeads { .. });
    if needs_repo && !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        std::process::exit(128);
    }
    let result = match action {
        BundleAction::Create { quiet, progress: _, version, file, revs } => bundle_create(quiet, version.unwrap_or(2), &file, &revs),
        BundleAction::Verify { quiet, file } => bundle_verify(quiet, &file),
        BundleAction::ListHeads { file, refnames } => read_bundle(&file).map(|(header, _)| print_refs(&header, &refnames)),
        BundleAction::Unbundle { progress: _, file, refnames } => bundle_unbundle(&file, &refnames),
    };
    if let Err(e) = result {
        let message = e.to_string();
        eprintln!("{}", message);
        std::process::exit(if message.starts_with("error:") { 1 } else { 128 });
    }
    Ok(())
}

fn bundle_create(quiet: bool, version: u8, file: &str, revs: &[String]) -> Result<()> {
    if version != 2 && version != 3 {
        anyhow::bail!("fatal: unsupported bundle version {}", version);
    }
    let (tips, negatives) = resolve_bundle_revs(revs)?;
    let (commits, prerequisites) = select_bundle_commits(&tips, &negatives)?;

    let mut refs: Vec<(String, String)> = Vec::new();
    let mut seen = HashSet::new();
    for tip in &tips {
        let Some(refname) = &tip.refname else { continue };
        if !seen.insert(refname.clone()) {
            continue;
        }
        if !commits.contains(&tip.commit) {
            if !negatives.contains(&tip.commit) {
                eprintln!("warning: ref '{}' is excluded by the rev-list options", refname);
            }
            continue;
        }
        refs.push((tip.object.clone(), refname.clone()));
    }
    if refs.is_empty() {
        anyhow::bail!("fatal: Refusing to create empty bundle.");
    }

    let objects = enumerate_bundle_objects(&commits, &prerequisites, &tips)?;
    let mut contents = format_bundle_header(version, &refs, &prerequisites)?.into_bytes();
    contents.extend(write_packfile(&objects)?);

    if file == "-" {
        io::stdout().lock().write_all(&contents)?;
    } else {
        let lock = format!("{}.lock", file);
        fs::write(&lock, &contents).map_err(|e| anyhow::anyhow!("fatal: could not create '{}': {}", file, e))?;
        fs::rename(&lock, file)?;
    }
    if !quiet && io::stderr().is_terminal() {
        eprintln!("Total {} (delta 0), reused 0 (delta 0), pack-reused 0", objects.len());
    }
    Ok(())
}

fn read_bundle(file: &str) -> Result<(BundleHeader, Vec<u8>)> {
    let bytes = fs::read(file).map_err(|_| anyhow::anyhow!("error: could not open '{}'", file))?;
    let header = parse_bundle_header(&bytes, file)?;
    Ok((header, bytes))
}

fn print_refs(header: &BundleHeader, filter: &[String]) {
    for (oid, name) in &header.refs {
        if filter.is_empty() || filter.contains(name) {
            println!("{} {}", oid, name);
        }
    }
}

fn check_prerequisites(header: &BundleHeader) -> Result<()> {
    let missing = missing_bundle_prerequisites(header);
    if missing.is_empty() {
        return Ok(());
    }
    let mut message = String::from("error: Repository lacks these prerequisite commits:");
    for oid in missing {
        message.push_str(&format!("\nerror: {} ", oid));
    }
    anyhow::bail!(message)
}

fn bundle_verify(quiet: bool, file: &str) -> Result<()> {
    let (header, _) = read_bundle(file)?;
    check_prerequisites(&header)?;
    if !quiet {
        let refs = header.refs.len();
        if refs == 1 {
            println!("The bundle contains this ref:");
        } else {
            println!("The bundle contains these {} refs:", refs);
        }
        print_refs(&header, &[]);
        match header.prerequisites.len() {
            0 => println!("The bundle records a complete history."),
            1 => println!("The bundle requires this ref:"),
            n => println!("The bundle requires these {} refs:", n),
        }
        for (oid, _) in &header.prerequisites {
            println!("{} ", oid);
        }
        println!("The bundle uses this hash algorithm: sha1");
        for (key, value) in &header.capabilities {
            if key == "filter" {
                println!("The bundle uses this filter: {}", value.as_deref().unwrap_or_default());
            }
        }
    }
    eprintln!("{} is okay", file);
    Ok(())
}

fn bundle_unbundle(file: &str, refnames: &[String]) -> Result<()> {
    let (header, bytes) = read_bundle(file)?;
    check_prerequisites(&header)?;
    let objects = read_packfile(&bytes[header.pack_offset..])?;
    let stored = store_loose_objects(&objects)?;
    if io::stderr().is_terminal() {
        eprintln!("Unpacking objects: 100% ({}/{}), done.", stored, stored);
    }
    print_refs(&header, refnames);
    Ok(())
}
