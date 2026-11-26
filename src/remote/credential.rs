pub mod cli;
pub mod engine;
pub mod types;

use std::io::Read;
use anyhow::Result;

use crate::remote::credential::engine::{approve, fill, parse_store_options, reject, store_action};
use crate::remote::credential::types::Credential;

const CREDENTIAL_USAGE: &str = "usage: rgit credential (fill|approve|reject)";
const STORE_USAGE: &str = "usage: rgit credential-store [<options>] <action>\n\n    --[no-]file <path>    fetch and store credentials in <path>\n";

/// `rgit credential (fill|approve|reject)`
///
/// Git's credential protocol: reads a credential (`protocol=`, `host=`, `path=`, `username=`,
/// `password=` or `url=` lines, up to a blank line) from stdin. `fill` asks every helper in
/// `credential.helper`, then `GIT_ASKPASS`/`core.askPass` or the terminal, for the missing
/// username and password and prints the completed credential; `approve` tells the helpers to
/// store it; `reject` tells them to erase it. Helpers written for git work unchanged.
/// rgit's GitHub commands use the same machinery for their token. Exits 128 on errors, 129
/// for usage errors.
pub fn credential(args: Vec<String>) -> Result<()> {
    let action = match args.as_slice() {
        [action] if matches!(action.as_str(), "fill" | "approve" | "reject") => action.as_str(),
        [flag] if flag == "-h" => crate::ui::die(129, CREDENTIAL_USAGE),
        _ => crate::ui::die(129, CREDENTIAL_USAGE),
    };
    let mut credential = read_credential_from_stdin();
    let result = match action {
        "fill" => fill(&mut credential, true).map(|_| ()),
        "approve" => approve(&credential),
        _ => reject(&credential),
    };
    if let Err(message) = result {
        crate::ui::die_fatal(message);
    }
    if action == "fill" {
        print!("{}", credential.to_protocol_text());
    }
    Ok(())
}

/// `rgit credential-store [--file <path>] (get|store|erase)`
///
/// The `store` credential helper (`credential.helper = store`): keeps logins unencrypted in
/// `~/.git-credentials` (readable by you only), one `https://user:password@host` line each,
/// in git's format, so git and rgit share it. Also reads `$XDG_CONFIG_HOME/git/credentials`;
/// `--file` uses that file instead. Normally run by `credential`, not by hand.
pub fn credential_store(args: Vec<String>) -> Result<()> {
    if args.iter().any(|a| a == "-h") {
        crate::ui::die(129, STORE_USAGE);
    }
    let (files, rest) = match parse_store_options(&args) {
        Ok(parsed) => parsed,
        Err(message) => crate::ui::die(129, &format!("error: {}\n{}", message, STORE_USAGE)),
    };
    let [action] = rest.as_slice() else { crate::ui::die(129, STORE_USAGE) };
    let credential = read_credential_from_stdin();
    if let Some(answer) = store_action(&files, action, &credential)? {
        print!("{}", answer.to_protocol_text());
    }
    Ok(())
}

/// Reads protocol input from stdin, dying like git on a line without `=`.
fn read_credential_from_stdin() -> Credential {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        crate::ui::die_fatal("unable to read credential from stdin");
    }
    Credential::parse(&input).unwrap_or_else(|line| {
        eprintln!("warning: invalid credential line: {}", line);
        crate::ui::die_fatal("unable to read credential from stdin")
    })
}
