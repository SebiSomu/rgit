//! The HTTP(S) client for git's smart-HTTP protocol: requests with a git User-Agent, the
//! 401 -> credential fill -> retry -> approve/reject login loop, and the safety rules around
//! logins (never over plain HTTP to another machine, never to a redirect's new host, never in
//! messages). `ls-remote`, fetch and push over HTTPS are built on it.

pub mod types;

use std::sync::Mutex;
use std::time::Duration;
use anyhow::Result;
use reqwest::blocking::{Client, Response};
use reqwest::{Method, StatusCode, Url};

use crate::remote::credential::types::Credential;
use crate::remote::http::types::{CredentialSource, HelperCredentials};

/// The User-Agent rgit sends: servers recognise git clients by the `git/` prefix.
pub const USER_AGENT: &str = concat!("git/rgit-", env!("CARGO_PKG_VERSION"));

/// How many redirects one request follows.
const MAX_REDIRECTS: usize = 10;

/// An HTTP client for one remote operation. It remembers the login that worked, so a fetch
/// that makes several requests asks for the credential once.
// Used by `ls-remote`, fetch and push over HTTPS (from step 4 on).
pub struct HttpClient {
    client: Client,
    credentials: Box<dyn CredentialSource>,
    allow_insecure_auth: bool,
    /// The login in use, once a server asked for one (or the URL carried one).
    login: Mutex<Option<Credential>>,
    /// Whether that login was already approved (helpers are told once per operation).
    approved: Mutex<bool>,
}

/// A response whose status was not 401: the caller checks the status (`check_status`) and
/// reads the body, all at once or as a stream (pack data).
// Used by `HttpClient` callers (from step 4 on).
pub struct HttpResponse {
    response: Response,
}

impl HttpResponse {
    /// The HTTP status.
    pub fn status(&self) -> StatusCode {
        self.response.status()
    }

    /// The `Content-Type` header, without parameters.
    pub fn content_type(&self) -> Option<String> {
        let value = self.response.headers().get(reqwest::header::CONTENT_TYPE)?.to_str().ok()?;
        Some(value.split(';').next().unwrap_or(value).trim().to_string())
    }

    /// The URL that answered, after redirects (git uses it as the remote's new base URL).
    pub fn final_url(&self) -> &Url {
        self.response.url()
    }

    /// Fails with git's messages unless the status is a success: `repository '<url>' not
    /// found` for 404, `unable to access '<url>': The requested URL returned error: <code>`
    /// otherwise. `repo_url` is the remote's URL as shown to the user.
    pub fn check_status(self, repo_url: &str) -> Result<HttpResponse> {
        let status = self.status();
        if status.is_success() {
            return Ok(self);
        }
        let shown = repo_display(repo_url);
        if status == StatusCode::NOT_FOUND {
            anyhow::bail!("fatal: repository '{}' not found", shown);
        }
        anyhow::bail!("fatal: unable to access '{}': The requested URL returned error: {}", shown, status.as_u16())
    }

    /// The whole body (ref lists and other small answers).
    pub fn bytes(self) -> Result<Vec<u8>> {
        Ok(self.response.bytes()?.to_vec())
    }

    /// The body as a stream (pack data).
    // Used by fetch over HTTPS from step 5 on.
    #[allow(dead_code)]
    pub fn into_reader(self) -> impl std::io::Read {
        self.response
    }
}

impl HttpClient {
    /// A client that asks the credential helpers and prompts for logins, and allows logins over
    /// plain HTTP to other machines only when `rgit.allowInsecureHttpAuth` is true.
    pub fn new() -> Result<HttpClient> {
        let allow = crate::core::config::get_config_value("rgit.allowinsecurehttpauth")?
            .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"));
        HttpClient::with_credentials(Box::new(HelperCredentials), allow)
    }

    /// A client with its own credential source (tests use a recording fake).
    pub fn with_credentials(credentials: Box<dyn CredentialSource>, allow_insecure_auth: bool) -> Result<HttpClient> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let downgrade = attempt.previous().iter().any(|u| u.scheme() == "https") && attempt.url().scheme() == "http";
                if downgrade {
                    attempt.error("refusing to follow a redirect from https to http")
                } else if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else {
                    // reqwest drops the Authorization header when the host, port or scheme changes.
                    attempt.follow()
                }
            }));
        // Unit tests talk to a server on 127.0.0.1; a proxy from the environment must not catch it.
        #[cfg(test)]
        let client = client.no_proxy();
        let client = client.build()?;
        Ok(HttpClient { client, credentials, allow_insecure_auth, login: Mutex::new(None), approved: Mutex::new(false) })
    }

    /// `GET url` with extra headers (e.g. `Git-Protocol: version=2`).
    pub fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse> {
        self.send(Method::GET, url, headers, None)
    }

    /// `POST url` with extra headers (`Content-Type`, `Accept`, `Git-Protocol`) and a body.
    pub fn post(&self, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<HttpResponse> {
        self.send(Method::POST, url, headers, Some(body))
    }

    /// Sends one request through the login loop: without a login first (unless one is known),
    /// and on a 401 once more with a login from the credential source. A login that is refused
    /// is rejected and the request fails; one that works is approved, once.
    fn send(&self, method: Method, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Result<HttpResponse> {
        let mut target = Url::parse(url).map_err(|e| anyhow::anyhow!("fatal: invalid URL '{}': {}", redact_url(url), e))?;
        let from_url = url_login(&target);
        let _ = target.set_username("");
        let _ = target.set_password(None);
        if from_url.is_some() && self.login.lock().unwrap().is_none() {
            *self.login.lock().unwrap() = from_url;
        }

        let mut filled_here = false;
        loop {
            let login = self.login.lock().unwrap().clone().filter(Credential::is_complete);
            if login.is_some() && !self.allow_insecure_auth && target.scheme() == "http" && !is_loopback(&target) {
                anyhow::bail!(
                    "fatal: refusing to send a password over unencrypted http to '{}'\nhint: use an https:// URL, or set rgit.allowInsecureHttpAuth=true",
                    target.host_str().unwrap_or_default()
                );
            }

            let mut request = self.client.request(method.clone(), target.clone());
            for (name, value) in headers {
                request = request.header(*name, *value);
            }
            if let Some(login) = &login {
                request = request.basic_auth(login.username.as_deref().unwrap_or_default(), login.password.as_deref());
            }
            if let Some(body) = body {
                request = request.body(body.to_vec());
            }
            let response = request
                .send()
                .map_err(|e| anyhow::anyhow!("fatal: unable to access '{}': {}", repo_display(url), describe_error(&e)))?;

            if response.status() != StatusCode::UNAUTHORIZED {
                if let Some(login) = &login
                    && response.status().is_success()
                {
                    let mut approved = self.approved.lock().unwrap();
                    if !*approved {
                        self.credentials.approve(login);
                        *approved = true;
                    }
                }
                return Ok(HttpResponse { response });
            }

            // 401: a login we already sent was refused, so forget it; otherwise ask for one.
            if let Some(login) = login {
                self.credentials.reject(&login);
                *self.login.lock().unwrap() = None;
                anyhow::bail!("fatal: Authentication failed for '{}'", repo_display(url));
            }
            if filled_here {
                anyhow::bail!("fatal: Authentication failed for '{}'", repo_display(url));
            }
            let mut wanted = credential_for(&target);
            if let Some(partial) = self.login.lock().unwrap().as_ref() {
                wanted.username = partial.username.clone();
            }
            self.credentials.fill(&mut wanted).map_err(|message| anyhow::anyhow!("fatal: {}", message))?;
            if !wanted.is_complete() {
                anyhow::bail!("fatal: Authentication failed for '{}'", repo_display(url));
            }
            *self.login.lock().unwrap() = Some(wanted);
            filled_here = true;
        }
    }
}

/// The credential to ask for: the protocol and `host[:port]` (git's credential fields).
// Used by `HttpClient::send`.
fn credential_for(url: &Url) -> Credential {
    let mut host = url.host_str().unwrap_or_default().to_string();
    if let Some(port) = url.port() {
        host = format!("{}:{}", host, port);
    }
    Credential { protocol: Some(url.scheme().to_string()), host: Some(host), ..Default::default() }
}

/// The login written in a URL (`https://user:password@host/...`), percent-decoded.
// Used by `HttpClient::send`.
fn url_login(url: &Url) -> Option<Credential> {
    if url.username().is_empty() {
        return None;
    }
    let decode = |s: &str| Credential::from_url(&format!("x://{}@h", s)).and_then(|c| c.username).unwrap_or_else(|| s.to_string());
    let mut credential = credential_for(url);
    credential.username = Some(decode(url.username()));
    credential.password = url.password().map(decode);
    Some(credential)
}

/// Whether the URL points at this machine, where plain HTTP can't be overheard.
// Used by `HttpClient::send`.
fn is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else { return false };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// The repository a request URL belongs to, as git names it in messages: no login, no query,
/// no protocol endpoint (`/info/refs`, `/git-upload-pack`, `/git-receive-pack`), and a
/// trailing slash (`https://host/owner/repo.git/`).
// Used by `HttpClient` and `HttpResponse::check_status`.
pub fn repo_display(url: &str) -> String {
    let shown = redact_url(url);
    let shown = shown.split_once('?').map(|(path, _)| path.to_string()).unwrap_or(shown);
    let mut shown = shown.trim_end_matches('/').to_string();
    for endpoint in ["/info/refs", "/git-upload-pack", "/git-receive-pack"] {
        if let Some(base) = shown.strip_suffix(endpoint) {
            shown = base.to_string();
            break;
        }
    }
    format!("{}/", shown)
}

/// `url` without any user or password in it, for messages.
// Used by `HttpClient` and `HttpResponse::check_status`.
pub fn redact_url(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        // Not a URL we can parse: drop everything between the scheme and an '@'.
        Err(_) => match (url.find("://"), url.rfind('@')) {
            (Some(scheme_end), Some(at)) if at > scheme_end => format!("{}{}", &url[..scheme_end + 3], &url[at + 1..]),
            _ => url.to_string(),
        },
    }
}

/// A short reason for a failed request (git prints curl's text in the same place).
// Used by `HttpClient::send`.
fn describe_error(error: &reqwest::Error) -> String {
    use std::error::Error;
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text = cause.to_string();
        source = cause.source();
    }
    text
}

#[cfg(test)]
#[path = "../../tests/unit/remote/http.rs"]
mod tests;
