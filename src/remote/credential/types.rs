/// One credential in git's credential protocol: the `key=value` description of a login
/// (`protocol`, `host`, optional `path`, `username`, `password`) that `credential fill`,
/// helpers and `credential-store` read and write. `quit` is a helper asking rgit to stop
/// asking anyone else.
// Used by: src/remote/credential.rs, src/remote/credential/engine.rs, src/remote/github.rs
// Used by command/helper: `credential`, `credential-store`, `fill`, `approve`, `reject`, and `resolve_github_token`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Credential {
    pub protocol: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub quit: bool,
}

impl Credential {
    /// A credential for `https://<host>`, the shape rgit asks for before talking to GitHub.
    pub fn for_https_host(host: &str) -> Credential {
        Credential { protocol: Some("https".to_string()), host: Some(host.to_string()), ..Default::default() }
    }

    /// Parses protocol input: `key=value` lines up to a blank line or the end. `url=` is split
    /// into its parts; unknown keys are ignored, as git does. `Err` carries the bad line.
    pub fn parse(text: &str) -> Result<Credential, String> {
        let mut credential = Credential::default();
        for line in text.lines() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.is_empty() {
                break;
            }
            let Some((key, value)) = line.split_once('=') else { return Err(line.to_string()) };
            let value = value.to_string();
            match key {
                "protocol" => credential.protocol = Some(value),
                "host" => credential.host = Some(value),
                "path" => credential.path = Some(value),
                "username" => credential.username = Some(value),
                "password" => credential.password = Some(value),
                "quit" => credential.quit = matches!(value.as_str(), "1" | "true" | "yes" | "on"),
                "url" => {
                    let Some(parsed) = Credential::from_url(&value) else { return Err(line.to_string()) };
                    credential = Credential { quit: credential.quit, ..parsed };
                }
                _ => {}
            }
        }
        Ok(credential)
    }

    /// Splits `protocol://[user[:password]@]host[/path]`, percent-decoding the user and
    /// password (the line format of `~/.git-credentials` and of `url=` input).
    pub fn from_url(url: &str) -> Option<Credential> {
        let (protocol, rest) = url.split_once("://")?;
        if protocol.is_empty() {
            return None;
        }
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path)) => (authority, (!path.is_empty()).then(|| path.to_string())),
            None => (rest, None),
        };
        let (userinfo, host) = match authority.rsplit_once('@') {
            Some((userinfo, host)) => (Some(userinfo), host),
            None => (None, authority),
        };
        let (username, password) = match userinfo {
            Some(info) => match info.split_once(':') {
                Some((user, pass)) => (Some(percent_decode(user)), Some(percent_decode(pass))),
                None => (Some(percent_decode(info)), None),
            },
            None => (None, None),
        };
        Some(Credential { protocol: Some(protocol.to_string()), host: Some(host.to_string()), path, username, password, quit: false })
    }

    /// The `~/.git-credentials` line for this credential: `protocol://user:password@host[/path]`,
    /// with the user and password percent-encoded.
    pub fn to_store_line(&self) -> String {
        let mut line = format!(
            "{}://{}:{}@{}",
            self.protocol.as_deref().unwrap_or_default(),
            percent_encode(self.username.as_deref().unwrap_or_default()),
            percent_encode(self.password.as_deref().unwrap_or_default()),
            self.host.as_deref().unwrap_or_default()
        );
        if let Some(path) = &self.path {
            line.push('/');
            line.push_str(path);
        }
        line
    }

    /// The protocol text for this credential: one `key=value` line per known field, in git's
    /// order. Helpers get it on stdin; `credential fill` prints it.
    pub fn to_protocol_text(&self) -> String {
        let mut text = String::new();
        for (key, value) in [("protocol", &self.protocol), ("host", &self.host), ("path", &self.path), ("username", &self.username), ("password", &self.password)] {
            if let Some(value) = value {
                text.push_str(&format!("{}={}\n", key, value));
            }
        }
        text
    }

    /// `protocol://[user@]host[/path]`, as shown in prompts ("Password for 'https://me@github.com': ").
    pub fn describe(&self, with_user: bool) -> String {
        let mut url = format!("{}://", self.protocol.as_deref().unwrap_or_default());
        if with_user && let Some(user) = &self.username {
            url.push_str(&percent_encode(user));
            url.push('@');
        }
        url.push_str(self.host.as_deref().unwrap_or_default());
        if let Some(path) = &self.path {
            url.push('/');
            url.push_str(path);
        }
        url
    }

    /// Whether `stored` answers this query: every field set here (protocol, host, path,
    /// username, and the password when `with_password`) is set to the same value there, as
    /// git's `credential_match`.
    pub fn matches(&self, stored: &Credential, with_password: bool) -> bool {
        let same = |want: &Option<String>, have: &Option<String>| want.is_none() || want == have;
        same(&self.protocol, &stored.protocol)
            && same(&self.host, &stored.host)
            && same(&self.path, &stored.path)
            && same(&self.username, &stored.username)
            && (!with_password || same(&self.password, &stored.password))
    }

    /// Fills in what a helper answered: the fields it set replace ours, and `quit` sticks.
    pub fn merge_from(&mut self, answer: Credential) {
        for (ours, theirs) in [(&mut self.username, answer.username), (&mut self.password, answer.password), (&mut self.path, answer.path)] {
            if theirs.is_some() {
                *ours = theirs;
            }
        }
        self.quit |= answer.quit;
    }

    /// Whether both halves of the login are known.
    pub fn is_complete(&self) -> bool {
        self.username.is_some() && self.password.is_some()
    }
}

/// Percent-encodes everything but RFC 3986 unreserved characters (git's `is_rfc3986_unreserved`).
fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02x}", byte));
        }
    }
    out
}

/// Decodes `%xx` escapes; malformed escapes are kept as they are.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
#[path = "../../../tests/unit/remote/credential/types.rs"]
mod tests;
