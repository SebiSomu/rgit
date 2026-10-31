use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, Write};
use anyhow::Context;
use crate::helpers::check_ref_format::check_refname_format;
use crate::helpers::read_object;
use crate::helpers::reflog::{delete_reflog, record_ref_change, NULL_HASH};
use crate::helpers::rev_parse::resolve_rev;
use crate::helpers::symbolic_ref::{check_ref_creation_conflicts, is_refname_safe, read_raw_ref_value, resolve_ref_unsafe};
use crate::objects::{PreparedRefUpdate, RawRefValue, RefUpdate, UpdateRefOptions, UpdateRefsState};
use crate::refs::{prune_empty_ref_dirs, remove_packed_ref};
use crate::repo;

/// git's usage text for `update-ref`, printed for `-h` and after option errors (exit 129).
pub const UPDATE_REF_USAGE: &str = "usage: rgit update-ref [<options>] -d <refname> [<old-val>]
   or: rgit update-ref [<options>]    <refname> <new-val> [<old-val>]
   or: rgit update-ref [<options>] --stdin [-z]

    -m <reason>           reason of the update
    -d                    delete the reference
    --no-deref            update <refname> not the one it points to
    --deref               opposite of --no-deref
    -z                    stdin has NUL-terminated arguments
    --[no-]stdin          read updates from stdin
    --[no-]create-reflog  create a reflog
";

/// Parses `update-ref` arguments like git's option parser: switches can be bundled (`-dz`),
/// `-m` takes the rest of its word or the next argument, options may follow the names, and
/// everything after `--` is a name. Errors carry the usage text and mean exit 129.
// Used by `update-ref`.
pub fn parse_update_ref_args(args: &[String]) -> anyhow::Result<UpdateRefOptions> {
    let mut opts = UpdateRefOptions::default();
    let usage_error = |message: String| anyhow::anyhow!("{}\n{}", message, UPDATE_REF_USAGE);

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--" => {
                opts.args.extend(args[i + 1..].iter().cloned());
                break;
            }
            "--no-deref" => opts.no_deref = true,
            "--deref" => opts.no_deref = false,
            "--stdin" => opts.stdin = true,
            "--no-stdin" => opts.stdin = false,
            "--create-reflog" => opts.create_reflog = true,
            "--no-create-reflog" => opts.create_reflog = false,
            _ if arg.starts_with("--") => {
                let name = arg[2..].split('=').next().unwrap_or("");
                return Err(usage_error(format!("error: unknown option `{}'", name)));
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                let switches = &arg[1..];
                for (pos, c) in switches.char_indices() {
                    match c {
                        'd' => opts.delete = true,
                        'z' => opts.nul_terminated = true,
                        'm' => {
                            let rest = &switches[pos + 1..];
                            if !rest.is_empty() {
                                opts.message = Some(rest.to_string());
                            } else if let Some(next) = args.get(i + 1) {
                                opts.message = Some(next.clone());
                                i += 1;
                            } else {
                                return Err(usage_error("error: switch `m' requires a value".to_string()));
                            }
                            break;
                        }
                        other => return Err(usage_error(format!("error: unknown switch `{}'", other))),
                    }
                }
            }
            _ => opts.args.push(arg.to_string()),
        }
        i += 1;
    }
    Ok(opts)
}

/// Turns an `update-ref` value into a full object id like git's `get_oid`: 40 hex digits are
/// taken as-is (even for objects that don't exist, which the update then rejects), anything
/// else goes through revision parsing (`main`, `HEAD~2`, `v1^{commit}`, ...).
// Used by `update-ref` (arguments and `--stdin` values).
pub fn get_object_id(spec: &str) -> Option<String> {
    if spec.len() == 40 && spec.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(spec.to_ascii_lowercase());
    }
    if spec.is_empty() {
        return None;
    }
    resolve_rev(spec).ok()
}

/// Queues one change into a transaction, rejecting bad ref names like git's
/// `ref_transaction_update`: a ref that gets a new value must pass the ref-format check (one
/// level allowed), a ref that is only deleted or verified just has to be a safe name.
// Used by `update-ref` (both modes).
pub fn queue_ref_update(transaction: &mut Vec<RefUpdate>, update: RefUpdate) -> Result<(), String> {
    let writes_value = update.new_hash.as_deref().is_some_and(|h| h != NULL_HASH);
    let name_ok = if writes_value { check_refname_format(&update.refname, true, false) } else { is_refname_safe(&update.refname) };
    if !name_ok {
        return Err(format!("refusing to update ref with bad name '{}'", update.refname));
    }
    transaction.push(update);
    Ok(())
}

/// The ref HEAD ends on when followed through symbolic refs (`refs/heads/main`), or `None`
/// when HEAD is detached; updates to that ref are logged in HEAD's reflog too.
// Used by `commit_ref_transaction`.
fn head_target() -> Option<String> {
    match read_raw_ref_value("HEAD") {
        RawRefValue::Symbolic(_) => resolve_ref_unsafe("HEAD", true, false).map(|r| r.name),
        _ => None,
    }
}

/// Checks a transaction the way git's `ref_transaction_prepare` does, without changing
/// anything: no ref may appear twice (directly, through a symbolic ref, or as HEAD next to the
/// branch HEAD is on), refs created together must not nest, every `<old-oid>` must match, new values must be existing objects
/// (commits for branches), and new refs must not clash with existing ones. Errors are git's
/// messages without the `fatal:`/`error:` prefix.
// Used by `update-ref` (both modes).
pub fn prepare_ref_transaction(updates: &[RefUpdate]) -> Result<Vec<PreparedRefUpdate>, String> {
    let mut names: Vec<&str> = updates.iter().map(|u| u.refname.as_str()).collect();
    names.sort_unstable();
    if let Some(pair) = names.windows(2).find(|w| w[0] == w[1]) {
        return Err(format!("multiple updates for ref '{}' not allowed", pair[0]));
    }

    let head = head_target();
    let mut affected: HashSet<String> = updates.iter().map(|u| u.refname.clone()).collect();
    let mut targets = Vec::with_capacity(updates.len());
    for update in updates {
        let resolved = resolve_ref_unsafe(&update.refname, true, false);
        let target = match &resolved {
            Some(r) if !update.no_deref => r.name.clone(),
            _ => update.refname.clone(),
        };
        if target != update.refname && !affected.insert(target.clone()) {
            return Err(format!(
                "multiple updates for '{}' (including one via symref '{}') are not allowed",
                target, update.refname
            ));
        }
        let logs_head = target != "HEAD" && head.as_deref() == Some(target.as_str());
        if logs_head && update.refname != "HEAD" && !affected.insert("HEAD".to_string()) {
            return Err(format!("multiple updates for 'HEAD' (including one via its referent '{}') are not allowed", target));
        }
        targets.push((target, resolved.and_then(|r| r.hash), logs_head));
    }

    let writes = |u: &RefUpdate| u.new_hash.as_deref().is_some_and(|h| h != NULL_HASH);
    for (i, (update, (target, _, _))) in updates.iter().zip(&targets).enumerate() {
        if !writes(update) {
            continue;
        }
        let nested = updates.iter().zip(&targets).enumerate().find(|(j, (other, (other_target, _, _)))| {
            *j != i
                && writes(other)
                && (other_target.starts_with(&format!("{}/", target)) || target.starts_with(&format!("{}/", other_target)))
        });
        if let Some((_, (_, (other_target, _, _)))) = nested {
            return Err(format!("cannot lock ref '{}': cannot process '{}' and '{}' at the same time", target, target, other_target));
        }
    }

    let mut prepared = Vec::with_capacity(updates.len());
    for (update, (target, current, logs_head)) in updates.iter().zip(targets) {
        if let Some(old) = &update.old_hash {
            let lock_error = |reason: String| format!("cannot lock ref '{}': {}", update.refname, reason);
            match current.as_deref() {
                Some(_) if old == NULL_HASH => return Err(lock_error("reference already exists".to_string())),
                None if old != NULL_HASH => {
                    return Err(format!("cannot lock ref '{}': unable to resolve reference '{}'", update.refname, target))
                }
                Some(cur) if cur != old => return Err(lock_error(format!("is at {} but expected {}", cur, old))),
                _ => {}
            }
        }

        if let Some(new) = update.new_hash.as_deref().filter(|h| *h != NULL_HASH && current.as_deref() != Some(*h)) {
            let update_error = |reason: String| format!("cannot update ref '{}': {}", target, reason);
            let object_type = match read_object(new) {
                Ok((object_type, _)) => object_type,
                Err(_) => return Err(update_error(format!("trying to write ref '{}' with nonexistent object {}", target, new))),
            };
            if object_type != "commit" && target.starts_with("refs/heads/") {
                return Err(update_error(format!("trying to write non-commit object {} to branch '{}'", new, target)));
            }
            if current.is_none() && target.starts_with("refs/") {
                check_ref_creation_conflicts(&target).map_err(|e| e.to_string().trim_start_matches("error: ").to_string())?;
            }
        }

        prepared.push(PreparedRefUpdate {
            target,
            current,
            new_hash: update.new_hash.clone(),
            create_reflog: update.create_reflog,
            logs_head,
        });
    }
    Ok(prepared)
}

/// Applies a prepared transaction: writes new values into the target ref files, deletes refs
/// (loose file, `packed-refs` entry and reflog), and logs every change with `message` in the
/// ref's reflog and, when the ref is the branch HEAD is on, in HEAD's reflog too. A ref whose
/// value doesn't change is left alone and not logged, but HEAD's reflog still gets the entry,
/// like git.
// Used by `update-ref` (both modes).
pub fn commit_ref_transaction(prepared: &[PreparedRefUpdate], message: &str) -> anyhow::Result<()> {
    for update in prepared {
        let Some(new) = update.new_hash.as_deref() else { continue };
        let logs_head = update.logs_head;
        let path = repo::path(format!(".git/{}", update.target));

        if new == NULL_HASH {
            if path.is_file() {
                fs::remove_file(&path).with_context(|| format!("unable to delete ref '{}'", update.target))?;
                prune_empty_ref_dirs(&path);
            }
            remove_packed_ref(&update.target)?;
            delete_reflog(&update.target);
            if logs_head && update.current.is_some() {
                record_ref_change("HEAD", update.current.as_deref(), None, message, false);
            }
            continue;
        }

        let overwrites_symref = matches!(read_raw_ref_value(&update.target), RawRefValue::Symbolic(_));
        if update.current.as_deref() == Some(new) && !overwrites_symref {
            if logs_head {
                record_ref_change("HEAD", Some(new), Some(new), message, false);
            }
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(&path, format!("{}\n", new)).with_context(|| format!("unable to write ref '{}'", update.target))?;

        record_ref_change(&update.target, update.current.as_deref(), Some(new), message, update.create_reflog);
        if logs_head {
            record_ref_change("HEAD", update.current.as_deref(), Some(new), message, false);
        }
    }
    Ok(())
}

/// Prints a fatal `update-ref --stdin` error and exits 128, like git's `die`.
fn die(message: impl AsRef<str>) -> ! {
    eprintln!("fatal: {}", message.as_ref());
    std::process::exit(128);
}

/// A position inside one `--stdin` command (with its terminator, and with `-z` its following
/// NUL-terminated arguments), read the way git walks its input buffer: past the end reads as
/// NUL, like the C string's terminator.
struct StdinCommand<'a> {
    buf: &'a [u8],
    pos: usize,
    term: u8,
}

impl StdinCommand<'_> {
    fn at(&self, pos: usize) -> u8 {
        self.buf.get(pos).copied().unwrap_or(0)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.buf.len()
    }

    /// The rest of the input from `pos` up to the next NUL, as git prints it in errors.
    fn rest_from(&self, pos: usize) -> String {
        let tail = &self.buf[pos.min(self.buf.len())..];
        let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
        String::from_utf8_lossy(&tail[..end]).into_owned()
    }

    /// Reads one argument without `-z`: a C-quoted string or a run of non-space characters.
    fn parse_arg(&mut self) -> Vec<u8> {
        let orig = self.pos;
        if self.at(self.pos) == b'"' {
            let Some((value, end)) = unquote_c_style(self.buf, self.pos) else {
                die(format!("badly quoted argument: {}", self.rest_from(orig)));
            };
            self.pos = end;
            if !self.at_end() && self.at(self.pos) != 0 && !self.at(self.pos).is_ascii_whitespace() {
                die(format!("unexpected character after quoted argument: {}", self.rest_from(orig)));
            }
            return value;
        }
        let start = self.pos;
        while !self.at_end() && self.at(self.pos) != 0 && !self.at(self.pos).is_ascii_whitespace() {
            self.pos += 1;
        }
        self.buf[start..self.pos].to_vec()
    }

    /// Reads a NUL-terminated field (`-z`), leaving `pos` on its terminator.
    fn parse_field(&mut self) -> Vec<u8> {
        let start = self.pos;
        while !self.at_end() && self.at(self.pos) != 0 {
            self.pos += 1;
        }
        self.buf[start..self.pos].to_vec()
    }

    /// git's `parse_refname`: the next argument (or field with `-z`), checked against the
    /// ref-format rules; `None` when it is empty.
    fn parse_refname(&mut self) -> Option<String> {
        let raw = if self.term != 0 { self.parse_arg() } else { self.parse_field() };
        if raw.is_empty() {
            return None;
        }
        let name = String::from_utf8_lossy(&raw).into_owned();
        if !check_refname_format(&name, true, false) {
            die(format!("invalid ref format: {}", name));
        }
        Some(name)
    }

    /// git's `parse_next_oid`: `Some(hash)` for a value (empty means the zero hash, with `-z`
    /// only where `allow_empty`), `None` when the argument is absent. Dies on bad values and on
    /// input that ends early.
    fn parse_next_oid(&mut self, command: &str, refname: &str, old: bool, allow_empty: bool) -> Option<String> {
        let what = if old { "<oldvalue>" } else { "<newvalue>" };
        let eof = || -> ! { die(format!("{} {}: unexpected end of input when reading {}", command, refname, what)) };
        if self.at_end() {
            eof();
        }
        let arg = if self.term != 0 {
            let c = self.at(self.pos);
            if c == 0 || c == self.term {
                return None;
            }
            if c != b' ' {
                die(format!("{} {}: expected SP but got: {}", command, refname, self.rest_from(self.pos)));
            }
            self.pos += 1;
            let arg = self.parse_arg();
            if arg.is_empty() {
                return Some(NULL_HASH.to_string());
            }
            arg
        } else {
            if self.at(self.pos) != 0 {
                die(format!("{} {}: expected NUL but got: {}", command, refname, self.rest_from(self.pos)));
            }
            self.pos += 1;
            if self.at_end() {
                eof();
            }
            let arg = self.parse_field();
            if arg.is_empty() {
                if allow_empty {
                    eprintln!("warning: {} {}: missing {}, treating as zero", command, refname, what);
                    return Some(NULL_HASH.to_string());
                }
                return None;
            }
            arg
        };
        let arg = String::from_utf8_lossy(&arg).into_owned();
        match get_object_id(&arg) {
            Some(hash) => Some(hash),
            None => die(format!("{} {}: invalid {}: {}", command, refname, what, arg)),
        }
    }

    /// Dies unless the command ends here (git's "extra input" check).
    fn expect_end(&self, command: &str, refname: &str) {
        if self.at(self.pos) != self.term {
            die(format!("{} {}: extra input: {}", command, refname, self.rest_from(self.pos)));
        }
    }
}

/// Decodes a C-style quoted string starting at `start` (the opening `"`), like git's
/// `unquote_c_style`: `\\`, `\"`, `\a\b\f\n\r\t\v` and three-digit octal escapes. Returns the
/// bytes and the position after the closing quote, or `None` for bad quoting.
// Used by `update-ref --stdin` (quoted ref names).
pub fn unquote_c_style(buf: &[u8], start: usize) -> Option<(Vec<u8>, usize)> {
    let mut out = Vec::new();
    let mut i = start + 1;
    loop {
        match *buf.get(i)? {
            b'"' => return Some((out, i + 1)),
            b'\\' => {
                i += 1;
                let c = *buf.get(i)?;
                let byte = match c {
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b'f' => 0x0c,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'v' => 0x0b,
                    b'\\' | b'"' => c,
                    b'0'..=b'3' => {
                        let digits = buf.get(i..i + 3)?;
                        if !digits.iter().all(|d| (b'0'..=b'7').contains(d)) {
                            return None;
                        }
                        i += 2;
                        (digits[0] - b'0') * 64 + (digits[1] - b'0') * 8 + (digits[2] - b'0')
                    }
                    _ => return None,
                };
                out.push(byte);
            }
            0 => return None,
            c => out.push(c),
        }
        i += 1;
    }
}

/// Runs `update-ref --stdin`: reads commands one at a time (so a client can wait for
/// `start: ok` / `prepare: ok` before sending more) and drives git's transaction states.
/// `update`/`create`/`delete`/`verify`/`option no-deref` queue changes, `start`, `prepare`,
/// `commit` and `abort` control an explicit transaction and answer `<cmd>: ok`. Input that
/// ends with an implicit transaction open commits it; an explicit one still open is aborted.
/// Any error is fatal (exit 128) and nothing is written.
// Used by `update-ref --stdin`.
pub fn run_update_ref_stdin(opts: &UpdateRefOptions) -> anyhow::Result<()> {
    const COMMANDS: &[(&str, usize, UpdateRefsState)] = &[
        ("update", 3, UpdateRefsState::Open),
        ("create", 2, UpdateRefsState::Open),
        ("delete", 2, UpdateRefsState::Open),
        ("verify", 2, UpdateRefsState::Open),
        ("option", 1, UpdateRefsState::Open),
        ("start", 0, UpdateRefsState::Started),
        ("prepare", 0, UpdateRefsState::Prepared),
        ("abort", 0, UpdateRefsState::Closed),
        ("commit", 0, UpdateRefsState::Closed),
    ];

    let term = if opts.nul_terminated { 0u8 } else { b'\n' };
    let message = opts.message.clone().unwrap_or_default();
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();

    let mut state = UpdateRefsState::Open;
    let mut transaction: Vec<RefUpdate> = Vec::new();
    let mut no_deref_next = false;

    loop {
        let mut buf = Vec::new();
        if input.read_until(term, &mut buf)? == 0 {
            break;
        }

        let found = COMMANDS.iter().find(|(prefix, args, _)| {
            buf.starts_with(prefix.as_bytes()) && buf.get(prefix.len()).copied().unwrap_or(0) == if *args > 0 { b' ' } else { term }
        });
        let Some(&(name, args, cmd_state)) = found else {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            die(format!("unknown command: {}", String::from_utf8_lossy(&buf[..end])));
        };
        if term == 0 {
            for _ in 1..args {
                if input.read_until(0, &mut buf)? == 0 {
                    break;
                }
            }
        }

        match state {
            UpdateRefsState::Open | UpdateRefsState::Started => {
                if state == UpdateRefsState::Started && cmd_state == UpdateRefsState::Started {
                    die("cannot restart ongoing transaction");
                }
                if cmd_state >= state {
                    state = cmd_state;
                }
            }
            UpdateRefsState::Prepared => {
                if cmd_state != UpdateRefsState::Closed {
                    die("prepared transactions can only be closed");
                }
                state = cmd_state;
            }
            UpdateRefsState::Closed => {
                if cmd_state != UpdateRefsState::Started {
                    die("transaction is closed");
                }
                state = cmd_state;
                transaction.clear();
            }
        }

        let mut cmd = StdinCommand { buf: &buf, pos: name.len() + usize::from(args > 0), term };
        let flags_no_deref = opts.no_deref || no_deref_next;
        match name {
            "update" | "create" | "delete" | "verify" => {
                let Some(refname) = cmd.parse_refname() else { die(format!("{}: missing <ref>", name)) };
                let (new_hash, old_hash) = match name {
                    "update" => {
                        let Some(new) = cmd.parse_next_oid(name, &refname, false, true) else {
                            die(format!("update {}: missing <newvalue>", refname))
                        };
                        (Some(new), cmd.parse_next_oid(name, &refname, true, false))
                    }
                    "create" => {
                        let Some(new) = cmd.parse_next_oid(name, &refname, false, false) else {
                            die(format!("create {}: missing <newvalue>", refname))
                        };
                        if new == NULL_HASH {
                            die(format!("create {}: zero <newvalue>", refname));
                        }
                        (Some(new), Some(NULL_HASH.to_string()))
                    }
                    "delete" => {
                        let old = cmd.parse_next_oid(name, &refname, true, false);
                        if old.as_deref() == Some(NULL_HASH) {
                            die(format!("delete {}: zero <oldvalue>", refname));
                        }
                        (Some(NULL_HASH.to_string()), old)
                    }
                    _ => {
                        let old = cmd.parse_next_oid(name, &refname, true, false).unwrap_or_else(|| NULL_HASH.to_string());
                        (None, Some(old))
                    }
                };
                cmd.expect_end(name, &refname);
                let update = RefUpdate {
                    refname,
                    new_hash,
                    old_hash,
                    no_deref: flags_no_deref,
                    create_reflog: opts.create_reflog && name != "delete" && name != "verify",
                };
                if let Err(e) = queue_ref_update(&mut transaction, update) {
                    die(e);
                }
                no_deref_next = false;
            }
            "option" => {
                let rest = &buf[cmd.pos.min(buf.len())..];
                let ok = rest.starts_with(b"no-deref") && cmd.at(cmd.pos + "no-deref".len()) == term;
                if !ok {
                    die(format!("option unknown: {}", cmd.rest_from(cmd.pos)));
                }
                no_deref_next = true;
            }
            "start" => println_flush(&stdout, "start: ok")?,
            "prepare" => {
                if let Err(e) = prepare_ref_transaction(&transaction) {
                    die(e);
                }
                println_flush(&stdout, "prepare: ok")?;
            }
            "commit" => {
                let prepared = prepare_ref_transaction(&transaction).unwrap_or_else(|e| die(e));
                commit_ref_transaction(&prepared, &message).unwrap_or_else(|e| die(format!("{:#}", e)));
                transaction.clear();
                println_flush(&stdout, "commit: ok")?;
            }
            _ => {
                transaction.clear();
                println_flush(&stdout, "abort: ok")?;
            }
        }
    }

    if state == UpdateRefsState::Open {
        let prepared = prepare_ref_transaction(&transaction).unwrap_or_else(|e| die(e));
        commit_ref_transaction(&prepared, &message).unwrap_or_else(|e| die(format!("{:#}", e)));
    }
    Ok(())
}

/// Prints one status line and flushes it right away, so an interactive `--stdin` client sees
/// it before sending its next command.
// Used by `run_update_ref_stdin`.
fn println_flush(stdout: &io::Stdout, line: &str) -> io::Result<()> {
    let mut out = stdout.lock();
    writeln!(out, "{}", line)?;
    out.flush()
}
