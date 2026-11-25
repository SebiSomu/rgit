//! Characterization-test harness: runs the real `rgit` binary inside a throwaway repository
//! and renders everything observable (stdout, stderr, exit codes, the `.git` state and the
//! worktree) into one text transcript that `insta` snapshots.
//!
//! Hashes are relabelled `<H1>`, `<H2>`, ... in order of first appearance, and "current time"
//! timestamps become `<NOW>`, so a transcript only changes when rgit's behaviour does.

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// First fixed author/committer timestamp; every command advances it by one minute.
const BASE_TIME: i64 = 1_700_000_000;

pub struct Repo {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    root: PathBuf,
    home: PathBuf,
    clock: i64,
    log: String,
    objects: BTreeSet<String>,
    env: BTreeMap<String, String>,
}

impl Repo {
    /// An empty directory (not yet a repository) with an isolated HOME.
    pub fn new() -> Repo {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path().canonicalize().expect("canonicalize tempdir");
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&home).unwrap();
        Repo { _tmp: tmp, base, root, home, clock: BASE_TIME, log: String::new(), objects: BTreeSet::new(), env: BTreeMap::new() }
    }

    /// A repository that already ran `rgit init`.
    pub fn init() -> Repo {
        let mut repo = Repo::new();
        repo.run("init");
        repo
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Sets an extra environment variable for every following command.
    pub fn set_env(&mut self, key: &str, value: &str) -> &mut Self {
        self.env.insert(key.to_string(), value.to_string());
        self.log.push_str(&format!("$ export {}={}\n", key, value));
        self
    }

    pub fn unset_env(&mut self, key: &str) -> &mut Self {
        self.env.remove(key);
        self.log.push_str(&format!("$ unset {}\n", key));
        self
    }

    /// Makes a file look `secs` seconds old (its modification time).
    pub fn age(&mut self, rel: &str, secs: u64) -> &mut Self {
        let file = fs::File::options().write(true).open(self.root.join(rel)).unwrap();
        file.set_modified(SystemTime::now() - std::time::Duration::from_secs(secs)).unwrap();
        self.log.push_str(&format!("$ age {} ({}s)\n", rel, secs));
        self
    }

    /// Writes a worktree file (parent directories are created).
    pub fn write(&mut self, rel: &str, content: &str) -> &mut Self {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        self.log.push_str(&format!("$ write {} ({} bytes)\n", rel, content.len()));
        self
    }

    pub fn remove(&mut self, rel: &str) -> &mut Self {
        let path = self.root.join(rel);
        if path.is_dir() { fs::remove_dir_all(&path).unwrap() } else { fs::remove_file(&path).unwrap() }
        self.log.push_str(&format!("$ remove {}\n", rel));
        self
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.root.join(rel)).unwrap_or_else(|e| format!("<unreadable: {}>", e))
    }

    /// Runs `rgit <args>` (split shell-style) in the repository root and logs the result.
    pub fn run(&mut self, args: &str) -> &mut Self {
        self.run_in(".", args)
    }

    /// Runs `rgit <args>` from a subdirectory of the repository root.
    pub fn run_in(&mut self, dir: &str, args: &str) -> &mut Self {
        self.run_with_stdin(dir, args, None)
    }

    /// Runs `rgit <args>` feeding `stdin` to it.
    pub fn run_stdin(&mut self, args: &str, stdin: &str) -> &mut Self {
        self.run_with_stdin(".", args, Some(stdin))
    }

    fn run_with_stdin(&mut self, dir: &str, args: &str, stdin: Option<&str>) -> &mut Self {
        use std::io::Write;
        use std::process::Stdio;

        let argv = split_args(args);
        let cwd = self.root.join(dir);
        let date = format!("{} +0000", self.clock);
        self.clock += 60;

        let prompt = if dir == "." { String::from("$") } else { format!("({}) $", dir) };
        if !cwd.is_dir() {
            self.log.push_str(&format!("{} rgit {}\n[skipped: directory {} does not exist]\n", prompt, args, dir));
            return self;
        }

        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rgit"));
        cmd.args(&argv).current_dir(&cwd);
        for (key, _) in std::env::vars_os() {
            let key = key.to_string_lossy().to_string();
            if key.starts_with("GIT_") || key.starts_with("RGIT_") || key.starts_with("RUST_") || key == "EDITOR" || key == "VISUAL" || key == "PAGER" || key == "XDG_CONFIG_HOME" {
                cmd.env_remove(&key);
            }
        }
        cmd.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("TZ", "UTC")
            .env("NO_COLOR", "1")
            .env("COLUMNS", "80")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LC_ALL", "C")
            .env("GIT_AUTHOR_NAME", "Test Author")
            .env("GIT_AUTHOR_EMAIL", "author@example.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_NAME", "Test Committer")
            .env("GIT_COMMITTER_EMAIL", "committer@example.com")
            .env("GIT_COMMITTER_DATE", &date);
        for (key, value) in &self.env {
            cmd.env(key, value);
        }
        cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().expect("spawn rgit");
        if let Some(input) = stdin {
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        }
        let out = child.wait_with_output().expect("wait rgit");

        self.log.push_str(&format!("{} rgit {}\n", prompt, args));
        if let Some(input) = stdin {
            self.log.push_str(&format!("<stdin>\n{}", ensure_newline(input)));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stdout.is_empty() {
            self.log.push_str(&ensure_newline(&stdout));
        }
        if !stderr.is_empty() {
            self.log.push_str("[stderr]\n");
            self.log.push_str(&ensure_newline(&stderr));
        }
        let code = out.status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".into());
        if code != "0" {
            self.log.push_str(&format!("[exit {}]\n", code));
        }
        self.collect_objects();
        self
    }

    /// Records every object id currently in the store, so abbreviations in output can be
    /// recognized and relabelled.
    fn collect_objects(&mut self) {
        let dir = self.root.join(".git").join("objects");
        let Ok(entries) = fs::read_dir(&dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.len() != 2 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            if let Ok(files) = fs::read_dir(entry.path()) {
                for file in files.flatten() {
                    let rest = file.file_name().to_string_lossy().to_string();
                    if rest.len() == 38 {
                        self.objects.insert(format!("{}{}", name, rest));
                    }
                }
            }
        }
    }

    /// The transcript plus a final state dump, normalized for snapshotting.
    pub fn finish(&mut self) -> String {
        let mut text = std::mem::take(&mut self.log);
        text.push_str("\n==== final state ====\n");
        text.push_str(&self.state_dump());
        self.normalize(&text)
    }

    /// The transcript plus a state dump, without consuming the log (for mid-scenario checkpoints).
    pub fn checkpoint(&mut self, label: &str) -> &mut Self {
        let dump = self.state_dump();
        self.log.push_str(&format!("\n==== state: {} ====\n{}\n", label, dump));
        self
    }

    fn state_dump(&mut self) -> String {
        let git = self.root.join(".git");
        let mut out = String::new();
        if !git.exists() {
            out.push_str("(no .git)\n");
        } else {
            out.push_str(&format!("HEAD: {}\n", read_trim(&git.join("HEAD"))));

            out.push_str("refs:\n");
            let mut refs = Vec::new();
            walk_files(&git.join("refs"), &git, &mut refs);
            refs.sort();
            for r in refs {
                out.push_str(&format!("  {} {}\n", r, read_trim(&git.join(&r))));
            }
            let packed = git.join("packed-refs");
            if packed.exists() {
                out.push_str("packed-refs:\n");
                for line in fs::read_to_string(&packed).unwrap_or_default().lines() {
                    out.push_str(&format!("  {}\n", line));
                }
            }

            out.push_str("git-dir entries:\n");
            let mut names: Vec<String> = fs::read_dir(&git).unwrap().flatten().map(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                if e.path().is_dir() { format!("{}/", n) } else { n }
            }).filter(|n| !matches!(n.as_str(), "objects/" | "refs/" | "HEAD" | "index" | "packed-refs" | "logs/" | "description" | "hooks/" | "info/" | "rgit/")).collect();
            names.sort();
            out.push_str(&format!("  {}\n", names.join(" ")));

            if git.join("index").exists() {
                out.push_str("index (ls-files -s):\n");
                let listing = self.quiet_rgit(&["ls-files", "-s"]);
                for line in listing.lines() {
                    out.push_str(&format!("  {}\n", line));
                }
            }

            let head_commit = self.quiet_rgit(&["rev-parse", "--verify", "-q", "HEAD"]);
            let head_commit = head_commit.trim();
            if !head_commit.is_empty() {
                out.push_str("HEAD commit:\n");
                for line in self.quiet_rgit(&["cat-file", "-p", head_commit]).lines() {
                    out.push_str(&format!("  {}\n", line));
                }
            }
            self.collect_objects();
        }

        out.push_str("worktree:\n");
        let mut files = Vec::new();
        walk_worktree(&self.root, &self.root, &mut files);
        files.sort();
        for f in files {
            let content = fs::read(self.root.join(&f)).unwrap_or_default();
            match std::str::from_utf8(&content) {
                Ok(text) if !text.contains('\0') => {
                    let lines: Vec<&str> = text.lines().collect();
                    out.push_str(&format!("  {}{}\n", f, if text.is_empty() { " (empty)" } else if !text.ends_with('\n') { " (no newline at end)" } else { "" }));
                    for line in lines.iter().take(20) {
                        out.push_str(&format!("    | {}\n", line));
                    }
                    if lines.len() > 20 {
                        out.push_str(&format!("    | ... {} more lines\n", lines.len() - 20));
                    }
                }
                // Zip headers record the host OS ("version made by"), so only the size is stable.
                _ if f.ends_with(".zip") => out.push_str(&format!("  {} (binary, {} bytes)\n", f, content.len())),
                _ => out.push_str(&format!("  {} (binary, {} bytes, fnv {:016x})\n", f, content.len(), fnv1a(&content))),
            }
        }
        out
    }

    fn quiet_rgit(&self, args: &[&str]) -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_rgit"))
            .args(args)
            .env_remove("RUST_BACKTRACE")
            .env_remove("RUST_LIB_BACKTRACE")
            .env_remove("XDG_CONFIG_HOME")
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("RGIT_VERBOSITY", "quiet")
            .env("NO_COLOR", "1")
            .env("COLUMNS", "80")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("spawn rgit");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn normalize(&self, text: &str) -> String {
        let mut text = text.replace("\r\n", "\n");
        let root_variants = path_variants(&self.root);
        let home_variants = path_variants(&self.home);
        for v in root_variants {
            text = text.replace(&v, "<REPO>");
        }
        for v in home_variants {
            text = text.replace(&v, "<HOME>");
        }
        for v in path_variants(&self.base) {
            text = text.replace(&v, "<TMP>");
        }
        text = platform_neutral(&text);
        text = relabel_hashes(&text, &self.objects);
        redact_now(&text)
    }
}

fn ensure_newline(s: &str) -> String {
    if s.ends_with('\n') { s.to_string() } else { format!("{}\n[no newline at end]\n", s) }
}

/// Stable content fingerprint for binary worktree files.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
}

fn read_trim(path: &Path) -> String {
    fs::read_to_string(path).map(|s| s.trim_end().to_string()).unwrap_or_else(|_| "<missing>".into())
}

fn walk_files(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files(&path, base, out);
        } else {
            out.push(path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

fn walk_worktree(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        if path.is_dir() {
            walk_worktree(&path, base, out);
        } else {
            out.push(path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

fn path_variants(path: &Path) -> Vec<String> {
    let raw = path.to_string_lossy().to_string();
    let mut v = vec![raw.clone(), raw.replace('\\', "/")];
    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        v.push(stripped.to_string());
        v.push(stripped.replace('\\', "/"));
    }
    v.sort_by_key(|s| std::cmp::Reverse(s.len()));
    v.dedup();
    v
}

/// Rewrites the Windows forms of output that differ only by platform into the Linux forms
/// the snapshots were recorded with: the binary name clap prints, OS error wording, and `\`
/// separators in paths under the temp placeholders.
fn platform_neutral(text: &str) -> String {
    static PATHS: OnceLock<Regex> = OnceLock::new();
    let mut text = text
        .replace("rgit.exe", "rgit")
        .replace("The system cannot find the path specified. (os error 3)", "No such file or directory (os error 2)")
        .replace("The system cannot find the file specified. (os error 2)", "No such file or directory (os error 2)");
    let re = PATHS.get_or_init(|| Regex::new(r#"<(?:REPO|HOME|TMP)>[^\s'"]*"#).unwrap());
    text = re.replace_all(&text, |caps: &regex::Captures| caps[0].replace('\\', "/")).to_string();
    text
}

fn hash_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b[0-9a-f]{7,40}\b").unwrap())
}

/// Replaces full object ids and their abbreviations (>= 7 hex digits that prefix exactly one
/// known object) with `<Hn>` labels; unknown 40-digit ids become `<Xn>`.
fn relabel_hashes(text: &str, objects: &BTreeSet<String>) -> String {
    let mut labels: HashMap<String, String> = HashMap::new();
    let mut next_h = 1;
    let mut next_x = 1;
    hash_re().replace_all(text, |caps: &regex::Captures| {
        let token = &caps[0];
        let matches: Vec<&String> = objects.iter().filter(|o| o.starts_with(token)).collect();
        let full = if matches.len() == 1 {
            matches[0].clone()
        } else if token.len() == 40 {
            token.to_string()
        } else {
            return token.to_string();
        };
        let label = labels.entry(full.clone()).or_insert_with(|| {
            if objects.contains(&full) {
                let l = format!("H{}", next_h);
                next_h += 1;
                l
            } else {
                let l = format!("X{}", next_x);
                next_x += 1;
                l
            }
        }).clone();
        if token.len() == 40 { format!("<{}>", label) } else { format!("<{}:{}>", label, token.len()) }
    }).to_string()
}

/// Unix timestamps within two days of now come from code that ignores GIT_*_DATE.
fn redact_now(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\b1[0-9]{9}\b").unwrap());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    let text = re.replace_all(text, |caps: &regex::Captures| {
        let n: i64 = caps[0].parse().unwrap();
        if (n - now).abs() < 2 * 86_400 { "<NOW>".to_string() } else { caps[0].to_string() }
    }).to_string();
    let text = redact_today(&text, now);
    // Ages ("3 years ago") are measured from the real clock, the dates from the fixed one.
    static AGO: OnceLock<Regex> = OnceLock::new();
    let ago = AGO.get_or_init(|| Regex::new(r"\b\d+ (?:second|minute|hour|day|month|year)s? ago\b").unwrap());
    ago.replace_all(&text, "<AGE> ago").to_string()
}

/// Human-readable renderings of today's date (UTC) come from the same clock-reading code.
fn redact_today(text: &str, now: i64) -> String {
    let days = now.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m - 1) as usize];
    let patterns = [
        format!(r"{:04}-{:02}-{:02}(?:[ T]\d\d:\d\d(?::\d\d)?(?: [+-]\d{{4}})?)?", y, m, d),
        format!(r"{} {} {} \d\d:\d\d:\d\d {}(?: [+-]\d{{4}})?", weekday, month, d, y),
        format!(r"{}, {} {} {} \d\d:\d\d:\d\d(?: [+-]\d{{4}})?", weekday, d, month, y),
    ];
    let mut out = text.to_string();
    for p in patterns {
        out = Regex::new(&p).unwrap().replace_all(&out, "<NOW-DATE>").to_string();
    }
    out
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Shell-like word splitting: whitespace separates, '...' and "..." group, \ escapes.
pub fn split_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some('"') if c == '\\' => {
                if let Some(n) = chars.next() { cur.push(n) }
            }
            Some(_) => cur.push(c),
            None => match c {
                '\'' | '"' => { quote = Some(c); in_word = true; }
                '\\' => { if let Some(n) = chars.next() { cur.push(n); in_word = true; } }
                c if c.is_whitespace() => {
                    if in_word { args.push(std::mem::take(&mut cur)); in_word = false; }
                }
                c => { cur.push(c); in_word = true; }
            },
        }
    }
    if in_word { args.push(cur); }
    args
}
