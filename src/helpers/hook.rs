use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use crate::helpers::config::get_config_value;

/// git's usage text for `hook`, printed for `-h` and after usage errors (exit 129).
pub const HOOK_USAGE: &str = "usage: rgit hook run [--ignore-missing] [--to-stdin=<path>] <hook-name> [-- <hook-args>]

    --ignore-missing      silently ignore missing requested <hook-name>
    --to-stdin <path>     file to read into hooks' stdin
";

/// Finds the hook script `name` like git's `find_hook`: in `core.hooksPath` when set,
/// otherwise in `.git/hooks`. `None` when there is no such file or (on Unix) it is not
/// executable, in which case git's hint about the ignored hook is printed.
// Used by `hook run`.
pub fn find_hook(name: &str) -> Option<PathBuf> {
    let dir = match get_config_value("core.hookspath").ok().flatten().filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => crate::repo::path(".git/hooks"),
    };
    let path = dir.join(name);
    let meta = fs::metadata(&path).ok().filter(|meta| meta.is_file())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            eprintln!("hint: The '{}' hook was ignored because it's not set as executable.", path.display());
            eprintln!("hint: You can disable this warning with `rgit config advice.ignoredHook false`.");
            return None;
        }
    }
    let _ = meta;
    Some(path)
}

/// Runs a hook script with `args` from the top of the working tree and returns its exit
/// status (1 when it could not be started or was killed). Its standard input is the file
/// `to_stdin`, or empty. On Windows the script is run through `sh`, as Git for Windows does.
// Used by `hook run`.
pub fn run_hook(hook: &PathBuf, args: &[String], to_stdin: Option<&str>) -> anyhow::Result<i32> {
    let stdin = match to_stdin {
        Some(path) => Stdio::from(fs::File::open(path).map_err(|e| anyhow::anyhow!("fatal: could not open '{}' for reading: {}", path, e))?),
        None => Stdio::null(),
    };
    let mut command = if cfg!(windows) {
        let mut command = Command::new("sh");
        command.arg(hook);
        command
    } else {
        Command::new(hook)
    };
    match command.args(args).stdin(stdin).status() {
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("error: cannot run {}: {}", hook.display(), e);
            Ok(1)
        }
    }
}
