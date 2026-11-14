use std::fs;
use std::io::{self, Read, Write};
use anyhow::Result;

use crate::helpers::fmt_merge_msg::{format_merge_message, parse_fmt_merge_msg_args, FMT_MERGE_MSG_USAGE};
use crate::repo;

/// `rgit fmt-merge-msg [-m <message>] [--into-name <branch>] [--log[=<n>] | --no-log] [-F <file>]`
///
/// Builds the default merge commit message from `FETCH_HEAD`-style lines read from standard
/// input (or `-F <file>`), each of the form `<hash>\t\tbranch 'topic' of <repository>`
/// (also `tag 'v1'`, `remote-tracking branch 'origin/x'`, or a bare repository for its
/// HEAD; `<hash>\tnot-for-merge\t...` lines are skipped). The title reads `Merge branch
/// 'topic' of <repository> into <current branch>`, joining several branches, tags and
/// repositories; ` into <branch>` is dropped for `main` and `master` (or the patterns in
/// `merge.suppressDest`), and `--into-name` names another branch. Tips already contained in
/// HEAD are ignored. `-m` replaces the title. Messages of merged annotated tags are
/// appended. `--log[=<n>]` (default from `merge.log`; 20 without a number) adds, per merged
/// tip, a `* topic:` list of the one-line subjects of at most `<n>` new commits, with `# By`
/// / `# Via` credit lines when other people authored or committed them, and the branch
/// description with `merge.branchdesc`. Exit status, like git: 0 success, 128 fatal (no
/// current branch, malformed line), 129 usage.
pub fn fmt_merge_msg(args: Vec<String>) -> Result<()> {
    if args.iter().any(|a| a == "-h") {
        print!("{}", FMT_MERGE_MSG_USAGE);
        std::process::exit(129);
    }
    if !repo::exists() {
        fatal("fatal: not a git repository (or any of the parent directories): .git");
    }
    let opts = match parse_fmt_merge_msg_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            let message = e.to_string();
            if message.starts_with("fatal:") || message.starts_with("error: merge.") {
                fatal(&message);
            }
            eprint!("{}", message);
            std::process::exit(129);
        }
    };

    let mut input = Vec::new();
    match opts.file.as_deref().filter(|path| *path != "-") {
        Some(path) => match fs::read(path) {
            Ok(bytes) => input = bytes,
            Err(e) => {
                let reason = if e.kind() == io::ErrorKind::NotFound { "No such file or directory".to_string() } else { e.to_string() };
                fatal(&format!("fatal: cannot open '{}': {}", path, reason));
            }
        },
        None => {
            if io::stdin().lock().read_to_end(&mut input).is_err() {
                fatal("fatal: could not read input file");
            }
        }
    }

    match format_merge_message(&input, &opts) {
        Ok(message) => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            out.write_all(&message)?;
            Ok(out.flush()?)
        }
        Err(e) => fatal(&e.to_string()),
    }
}

fn fatal(message: &str) -> ! {
    eprintln!("{}", message);
    std::process::exit(128);
}
