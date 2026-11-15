use crate::objects::{MergeConflictStyle, MergeFavor, MergeFileArgs};

/// Usage text of `merge-file`, printed for `-h` and usage errors.
pub const MERGE_FILE_USAGE: &str = "usage: rgit merge-file [<options>] [-L <name1> [-L <orig> [-L <name2>]]] <file1> <orig-file> <file2>\n\n    -p, --[no-]stdout     send results to standard output\n    --[no-]object-id      use object IDs instead of filenames\n    --[no-]diff3          use a diff3 based merge\n    --[no-]zdiff3         use a zealous diff3 based merge\n    --[no-]ours           for conflicts, use our version\n    --[no-]theirs         for conflicts, use their version\n    --[no-]union          for conflicts, use a union version\n    --[no-]marker-size <n>\n                          for conflicts, use this marker size\n    -q, --[no-]quiet      do not warn about conflicts\n    -L <name>             set labels for file1/orig-file/file2\n\n";

/// Parses the arguments of `merge-file`; `style` starts from `merge.conflictStyle`. Errors are
/// the text to print before exiting 129.
// Used by `merge-file`.
pub fn parse_merge_file_args(args: &[String], style: MergeConflictStyle) -> Result<MergeFileArgs, String> {
    let mut opts = MergeFileArgs { stdout: false, object_id: false, quiet: false, style, favor: MergeFavor::None, marker_size: 7, labels: Vec::new(), files: Vec::new() };
    let usage = |message: String| format!("{}\n{}", message, MERGE_FILE_USAGE);
    let mut options_done = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if options_done || !arg.starts_with('-') || arg == "-" {
            opts.files.push(arg.to_string());
            continue;
        }
        match arg {
            "--" => options_done = true,
            "-h" => return Err(MERGE_FILE_USAGE.to_string()),
            "-p" | "--stdout" => opts.stdout = true,
            "--no-stdout" => opts.stdout = false,
            "--object-id" => opts.object_id = true,
            "--no-object-id" => opts.object_id = false,
            "--diff3" => opts.style = MergeConflictStyle::Diff3,
            "--zdiff3" => opts.style = MergeConflictStyle::Zdiff3,
            "--no-diff3" | "--no-zdiff3" => opts.style = MergeConflictStyle::Merge,
            "--ours" => opts.favor = MergeFavor::Ours,
            "--theirs" => opts.favor = MergeFavor::Theirs,
            "--union" => opts.favor = MergeFavor::Union,
            "--no-ours" | "--no-theirs" | "--no-union" => opts.favor = MergeFavor::None,
            "-q" | "--quiet" => opts.quiet = true,
            "--no-quiet" => opts.quiet = false,
            "--no-marker-size" => opts.marker_size = 7,
            _ => {
                let (name, inline) = if let Some(rest) = arg.strip_prefix("-L") {
                    ("L", (!rest.is_empty()).then(|| rest.to_string()))
                } else if let Some(rest) = arg.strip_prefix("--marker-size") {
                    match rest.strip_prefix('=') {
                        Some(value) => ("marker-size", Some(value.to_string())),
                        None if rest.is_empty() => ("marker-size", None),
                        None => return Err(usage(format!("error: unknown option `{}'", &arg[2..]))),
                    }
                } else if let Some(long) = arg.strip_prefix("--") {
                    return Err(usage(format!("error: unknown option `{}'", long)));
                } else {
                    return Err(usage(format!("error: unknown switch `{}'", arg.chars().nth(1).unwrap_or('-'))));
                };
                let value = match inline {
                    Some(value) => value,
                    None => {
                        let next = args.get(i).cloned().ok_or_else(|| {
                            if name == "L" { "error: switch `L' requires a value\n".to_string() } else { format!("error: option `{}' requires a value\n", name) }
                        })?;
                        i += 1;
                        next
                    }
                };
                if name == "L" {
                    if opts.labels.len() >= 3 {
                        return Err("error: too many labels on the command line\n".to_string());
                    }
                    opts.labels.push(value);
                } else {
                    opts.marker_size = value.parse::<usize>().map_err(|_| "error: option `marker-size' expects a numerical value\n".to_string())?;
                }
            }
        }
    }
    if opts.files.len() != 3 {
        return Err(MERGE_FILE_USAGE.to_string());
    }
    Ok(opts)
}
