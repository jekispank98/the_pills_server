use reqwest::Url;

/// A Sentry-protocol DSN: `https://<public_key>@<host>[:port][/<path>]/<project_id>`.
/// Sentry, GlitchTip and Hawk all accept it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dsn {
    public_key: String,
    envelope_url: Url,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DsnError {
    #[error("not a valid URL")]
    InvalidUrl,
    #[error("the public key is missing")]
    MissingPublicKey,
    #[error("the project id is missing")]
    MissingProjectId,
}

impl Dsn {
    pub fn parse(raw: &str) -> Result<Self, DsnError> {
        let url = Url::parse(raw.trim()).map_err(|_| DsnError::InvalidUrl)?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(DsnError::InvalidUrl);
        }
        let public_key = url.username();
        if public_key.is_empty() {
            return Err(DsnError::MissingPublicKey);
        }

        let path = url.path().trim_end_matches('/');
        let (prefix, project_id) = path.rsplit_once('/').ok_or(DsnError::MissingProjectId)?;
        if project_id.is_empty() {
            return Err(DsnError::MissingProjectId);
        }

        let mut envelope_url = url.clone();
        envelope_url
            .set_username("")
            .map_err(|_| DsnError::InvalidUrl)?;
        envelope_url
            .set_password(None)
            .map_err(|_| DsnError::InvalidUrl)?;
        envelope_url.set_query(None);
        envelope_url.set_fragment(None);
        envelope_url.set_path(&format!("{prefix}/api/{project_id}/envelope/"));

        Ok(Self {
            public_key: public_key.to_string(),
            envelope_url,
        })
    }

    pub fn envelope_url(&self) -> &Url {
        &self.envelope_url
    }

    pub fn auth_header(&self, client: &str) -> String {
        format!(
            "Sentry sentry_version=7, sentry_key={}, sentry_client={client}",
            self.public_key
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_glitchtip_style_dsn() {
        let dsn = Dsn::parse("https://abc123@errors.example.ru/7").unwrap();
        assert_eq!(
            dsn.envelope_url().as_str(),
            "https://errors.example.ru/api/7/envelope/"
        );
        assert_eq!(
            dsn.auth_header("test/1"),
            "Sentry sentry_version=7, sentry_key=abc123, sentry_client=test/1"
        );
    }

    #[test]
    fn keeps_port_and_path_prefix() {
        let dsn = Dsn::parse("http://key@127.0.0.1:9000/glitchtip/42/").unwrap();
        assert_eq!(
            dsn.envelope_url().as_str(),
            "http://127.0.0.1:9000/glitchtip/api/42/envelope/"
        );
    }

    #[test]
    fn rejects_incomplete_dsns() {
        assert_eq!(Dsn::parse("not a url"), Err(DsnError::InvalidUrl));
        assert_eq!(
            Dsn::parse("https://errors.example.ru/7"),
            Err(DsnError::MissingPublicKey)
        );
        assert_eq!(
            Dsn::parse("https://key@errors.example.ru/"),
            Err(DsnError::MissingProjectId)
        );
    }
}
