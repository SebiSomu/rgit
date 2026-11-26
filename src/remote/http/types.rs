use crate::remote::credential::types::Credential;

/// Where `HttpClient` gets logins when a server answers 401: normally the credential helpers
/// and prompts (`HelperCredentials`); a recording fake in tests.
// Used by: src/remote/http.rs
// Used by command/helper: `HttpClient` (and from step 4 on, `ls-remote`, fetch and push over HTTPS)
pub trait CredentialSource: Send + Sync {
    /// Completes `credential` (protocol and host are set) with a username and password, or
    /// fails with a `fatal:` message without the prefix.
    fn fill(&self, credential: &mut Credential) -> Result<(), String>;
    /// The login worked: keep it.
    fn approve(&self, credential: &Credential);
    /// The login was refused: forget it.
    fn reject(&self, credential: &Credential);
}

/// The real `CredentialSource`: `credential.helper`, then `GIT_ASKPASS` / `core.askPass` or
/// the terminal, exactly as `rgit credential fill` does (failing like git when prompts are off).
// Used by: src/remote/http.rs
// Used by command/helper: `HttpClient::new`
pub struct HelperCredentials;

impl CredentialSource for HelperCredentials {
    fn fill(&self, credential: &mut Credential) -> Result<(), String> {
        crate::remote::credential::engine::fill(credential, true).map(|_| ())
    }

    fn approve(&self, credential: &Credential) {
        let _ = crate::remote::credential::engine::approve(credential);
    }

    fn reject(&self, credential: &Credential) {
        let _ = crate::remote::credential::engine::reject(credential);
    }
}
