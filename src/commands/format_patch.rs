use std::fs;
use std::path::Path;
use anyhow::{Context, Result};

use crate::helpers::format_patch::{diff_commit_against_parent, format_diffstat, format_mail_headers, patch_file_name, select_patch_commits, split_commit_message};
use crate::objects::FormatPatchOptions;

/// `rgit format-patch [-<n>] [--root] [--stdout | -o <dir>] [-n | -N] [--start-number <n>]
/// [--subject-prefix <p>] [-k] [--signature <s> | --no-signature] [<since> | <revision-range>]`
///
/// Turns commits into mailbox-format patches, one per commit, oldest first: mbox `From` line,
/// `From:`/`Date:`/`Subject: [PATCH n/m]` headers (RFC 2047-encoded when not ASCII), the commit
/// message, a `---` diffstat with creation/deletion/mode summary, the full git diff (including
/// binary literals), and a `-- ` signature. Writes `NNNN-<subject>.patch` files (printing their
/// names) or, with `--stdout`, one mailbox on standard output. Merge commits are skipped;
/// commits that change nothing get a patch without a diff, like git. The output is accepted by
/// `rgit am` and `git am`.
pub fn format_patch(mut opts: FormatPatchOptions) -> Result<()> {
    let mut revs = Vec::new();
    for rev in &opts.revs {
        match rev.strip_prefix('-').and_then(|n| n.parse::<usize>().ok()) {
            Some(n) => opts.max_count = Some(n),
            None => revs.push(rev.clone()),
        }
    }
    if revs.is_empty() && opts.max_count.is_none() && !opts.root {
        anyhow::bail!("fatal: no revision given (use '<since>', '<rev-range>', '-<n>', or '--root')");
    }

    let mut patches = Vec::new();
    for commit in select_patch_commits(&revs, opts.max_count, opts.root)? {
        let diffs = diff_commit_against_parent(&commit)?;
        patches.push((commit, diffs));
    }

    let start = opts.start_number.max(1);
    let total = patches.len() + start - 1;
    let numbered = !opts.keep_subject && !opts.no_numbered && (opts.numbered || patches.len() > 1);
    let signature = if opts.no_signature {
        None
    } else {
        Some(opts.signature.clone().unwrap_or_else(|| format!("rgit {}", env!("CARGO_PKG_VERSION"))))
    };

    if let Some(dir) = &opts.output_directory {
        if !opts.stdout {
            fs::create_dir_all(dir).with_context(|| format!("fatal: could not create directory '{}'", dir))?;
        }
    }

    for (i, (commit, diffs)) in patches.iter().enumerate() {
        let number = start + i;
        let (subject, body) = split_commit_message(&commit.message);
        let prefix = if opts.keep_subject {
            String::new()
        } else if numbered {
            let pad = total.to_string().len();
            let label = if opts.subject_prefix.is_empty() { String::new() } else { format!("{} ", opts.subject_prefix) };
            format!("[{}{:0pad$}/{}] ", label, number, total, pad = pad)
        } else if opts.subject_prefix.is_empty() {
            String::new()
        } else {
            format!("[{}] ", opts.subject_prefix)
        };

        let mut mail = format_mail_headers(commit, &prefix, &subject, &body);
        mail.push('\n');
        if !body.is_empty() {
            mail.push_str(&body);
            mail.push('\n');
        }
        if !diffs.is_empty() {
            mail.push_str("---\n");
            let stats: Vec<_> = diffs.iter().map(|d| d.stat.clone()).collect();
            mail.push_str(&format_diffstat(&stats, 72));
            for diff in diffs {
                if let Some(summary) = &diff.summary {
                    mail.push_str(summary);
                }
            }
            mail.push('\n');
            for diff in diffs {
                mail.push_str(&diff.text);
            }
        }
        if let Some(sig) = &signature {
            let newline = if sig.ends_with('\n') { "" } else { "\n" };
            mail.push_str(&format!("-- \n{}{}\n", sig, newline));
        }

        if opts.stdout {
            if i > 0 {
                println!();
            }
            print!("{}", mail);
        } else {
            let name = patch_file_name(number, &subject);
            let path = match &opts.output_directory {
                Some(dir) => format!("{}/{}", dir.trim_end_matches(['/', '\\']), name),
                None => name,
            };
            fs::write(Path::new(&path), mail).with_context(|| format!("fatal: could not write '{}'", path))?;
            println!("{}", path);
        }
    }

    Ok(())
}
