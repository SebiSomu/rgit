pub mod atoms;
pub mod color;
pub mod dates;
pub mod message;
pub mod options;
pub mod output;
pub mod refs;
pub mod sort;
pub mod trailers;
pub mod upstream;
pub mod values;

use crate::references::for_each_ref::types::ForEachRefExit;

/// git's `--format` default for `for-each-ref`.
pub const FOR_EACH_REF_DEFAULT_FORMAT: &str = "%(objectname) %(objecttype)\t%(refname)";

/// git's `for-each-ref -h` text, also printed after unknown options (exit 129).
pub const FOR_EACH_REF_USAGE: &str = "usage: rgit for-each-ref [<options>] [<pattern>]
   or: rgit for-each-ref [--points-at <object>]
   or: rgit for-each-ref [--merged [<commit>]] [--no-merged [<commit>]]
   or: rgit for-each-ref [--contains [<commit>]] [--no-contains [<commit>]]

    -s, --[no-]shell      quote placeholders suitably for shells
    -p, --[no-]perl       quote placeholders suitably for perl
    --[no-]python         quote placeholders suitably for python
    --[no-]tcl            quote placeholders suitably for Tcl
    --[no-]omit-empty     do not output a newline after empty formatted refs

    --[no-]count <n>      show only <n> matched refs
    --[no-]format <format>
                          format to use for the output
    --[no-]color[=<when>] respect format colors
    --[no-]exclude <pattern>
                          exclude refs which match pattern
    --[no-]sort <key>     field name to sort on
    --[no-]points-at <object>
                          print only refs which points at the given object
    --merged <commit>     print only refs that are merged
    --no-merged <commit>  print only refs that are not merged
    --contains <commit>   print only refs which contain the commit
    --no-contains <commit>
                          print only refs which don't contain the commit
    --[no-]ignore-case    sorting and filtering are case insensitive
    --[no-]stdin          read reference patterns from stdin
";

const LONG_OPTIONS: &[(&str, bool)] = &[
    ("shell", true),
    ("perl", true),
    ("python", true),
    ("tcl", true),
    ("omit-empty", true),
    ("count", true),
    ("format", true),
    ("color", true),
    ("exclude", true),
    ("sort", true),
    ("points-at", true),
    ("merged", false),
    ("no-merged", false),
    ("contains", false),
    ("no-contains", false),
    ("ignore-case", true),
    ("stdin", true),
];

const PER_WORKTREE_REFS: &[&str] = &["refs/bisect", "refs/worktree", "refs/rewritten"];
const SHORTEN_RULES: &[(&str, &str)] =
    &[("", ""), ("refs/", ""), ("refs/tags/", ""), ("refs/heads/", ""), ("refs/remotes/", ""), ("refs/remotes/", "/HEAD")];
const COLOR_RESET: &str = "\x1b[m";

fn exit_error(code: i32, message: String) -> ForEachRefExit {
    ForEachRefExit { code, stdout: String::new(), stderr: message }
}

fn usage_error(message: String) -> ForEachRefExit {
    exit_error(129, format!("{}\n{}", message, FOR_EACH_REF_USAGE))
}

fn fatal(message: String) -> ForEachRefExit {
    exit_error(128, format!("fatal: {}", message))
}
