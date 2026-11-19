pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;

use crate::core::reflog::NULL_HASH;
use crate::references::update_ref::engine::{
    commit_ref_transaction, get_object_id, parse_update_ref_args, prepare_ref_transaction, queue_ref_update,
    run_update_ref_stdin, UPDATE_REF_USAGE,
};
use crate::references::update_ref::types::{RefUpdate, UpdateRefOptions};
use crate::core::repo;

/// `rgit update-ref [-m <reason>] [--no-deref] [--create-reflog] <ref> <new-oid> [<old-oid>]`
/// `rgit update-ref [-m <reason>] [--no-deref] -d <ref> [<old-oid>]`
/// `rgit update-ref [-m <reason>] [--no-deref] [--create-reflog] --stdin [-z]`
///
/// Safely sets or deletes any ref. Values go through revision parsing (`main~2`, tags, ...),
/// and with `<old-oid>` the change only happens if the ref currently holds that value (an
/// empty `<old-oid>` means it must not exist yet). A symbolic ref such as HEAD is followed to
/// the ref it points at unless `--no-deref`. New values must be existing objects, and
/// branches only take commits. Changes are logged in the ref's reflog with the `-m` message
/// (and in HEAD's when the ref is the current branch); a deletion also removes the ref from
/// `packed-refs` and drops its reflog. `--stdin` applies a whole transaction of `update`,
/// `create`, `delete` and `verify` commands atomically, with `start`/`prepare`/`commit`/
/// `abort` for explicit control (`-z` for NUL-separated input). Exit status, like git: 0
/// success, 1 a failed `-d`, 128 a failed update or any `--stdin` error, 129 usage.
pub fn update_ref(args: Vec<String>) -> Result<()> {
    let before_dashdash = args.iter().take_while(|a| a.as_str() != "--");
    if before_dashdash.clone().any(|a| a == "-h" || a == "--help") {
        print!("{}", UPDATE_REF_USAGE);
        crate::ui::exit(129);
    }

    let opts = match parse_update_ref_args(&args) {
        Ok(opts) => opts,
        Err(e) => crate::ui::die_raw(129, &e.to_string()),
    };
    if !repo::exists() {
        crate::ui::die_fatal("not a git repository (or any of the parent directories): .git");
    }
    if opts.message.as_deref() == Some("") {
        crate::ui::die_fatal("Refusing to perform update with empty message.");
    }

    if opts.stdin {
        if opts.delete || !opts.args.is_empty() {
            crate::ui::die_raw(129, UPDATE_REF_USAGE);
        }
        return run_update_ref_stdin(&opts);
    }
    if opts.nul_terminated {
        crate::ui::die_raw(129, UPDATE_REF_USAGE);
    }
    run_single_update(&opts);
    Ok(())
}

/// The argument form: one update or deletion. Failed deletions are reported as `error:` with
/// exit 1, failed updates as `fatal: update_ref failed for ref ...` with exit 128, like git.
fn run_single_update(opts: &UpdateRefOptions) {
    let arg_count = opts.args.len();
    let valid_count = if opts.delete { (1..=2).contains(&arg_count) } else { (2..=3).contains(&arg_count) };
    if !valid_count {
        crate::ui::die_raw(129, UPDATE_REF_USAGE);
    }

    let refname = opts.args[0].clone();
    let (new_hash, old_arg) = if opts.delete {
        (NULL_HASH.to_string(), opts.args.get(1))
    } else {
        let value = &opts.args[1];
        let new = get_object_id(value).unwrap_or_else(|| crate::ui::die_fatal(format!("{}: not a valid SHA1", value)));
        (new, opts.args.get(2))
    };
    let old_hash = old_arg.map(|old| {
        if old.is_empty() {
            NULL_HASH.to_string()
        } else {
            get_object_id(old).unwrap_or_else(|| crate::ui::die_fatal(format!("{}: not a valid old SHA1", old)))
        }
    });
    // For a deletion the zero hash means "don't care", for backwards compatibility.
    let old_hash = if opts.delete { old_hash.filter(|h| h != NULL_HASH) } else { old_hash };

    let update = RefUpdate {
        refname: refname.clone(),
        new_hash: Some(new_hash),
        old_hash,
        no_deref: opts.no_deref,
        create_reflog: opts.create_reflog && !opts.delete,
    };
    let message = opts.message.clone().unwrap_or_default();
    let mut transaction = Vec::new();
    let result = queue_ref_update(&mut transaction, update)
        .and_then(|()| prepare_ref_transaction(&transaction))
        .and_then(|prepared| commit_ref_transaction(&prepared, &message).map_err(|e| format!("{:#}", e)));

    if let Err(e) = result {
        if opts.delete {
            eprintln!("error: {}", e);
            crate::ui::exit(1);
        }
        crate::ui::die_fatal(format!("update_ref failed for ref '{}': {}", refname, e));
    }
}
