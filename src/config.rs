use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_max_age: Duration,
    pub listen_addr: SocketAddr,
    /// Внешний адрес сервиса — из него собираются ссылки в письмах.
    pub public_base_url: String,
    pub google_web_client_id: String,
    pub smtp: SmtpConfig,
    pub env: Environment,
}

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub server: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from_address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Development,
    Production,
}

impl Environment {
    pub fn is_production(self) -> bool {
        self == Environment::Production
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("переменная окружения {0} не задана")]
    Missing(&'static str),
    #[error("переменная окружения {name} имеет некорректное значение: {source}")]
    Invalid {
        name: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let env = match optional("APP_ENV").as_deref() {
            Some("production") | Some("prod") => Environment::Production,
            _ => Environment::Development,
        };

        let host = optional("HOST").unwrap_or_else(|| "127.0.0.1".to_string());
        let port = parse_or("PORT", 3000u16)?;
        let listen_addr = format!("{host}:{port}")
            .parse()
            .map_err(|e: std::net::AddrParseError| ConfigError::Invalid {
                name: "HOST",
                source: Box::new(e),
            })?;

        let public_base_url = optional("PUBLIC_BASE_URL")
            .unwrap_or_else(|| format!("http://{listen_addr}"))
            .trim_end_matches('/')
            .to_string();

        let jwt_secret = required("JWT_SECRET_KEY")?;
        if jwt_secret.len() < 32 && env.is_production() {
            return Err(ConfigError::Invalid {
                name: "JWT_SECRET_KEY",
                source: "в production секрет должен быть не короче 32 байт".into(),
            });
        }

        Ok(Config {
            database_url: required("DATABASE_URL")?,
            jwt_secret,
            jwt_max_age: Duration::from_secs(60 * parse_or("JWT_MAXAGE", 60u64)?),
            listen_addr,
            public_base_url,
            google_web_client_id: required("GOOGLE_WEB_CLIENT_ID")?,
            smtp: SmtpConfig {
                server: required("SMTP_SERVER")?,
                port: parse_or("SMTP_PORT", 587u16)?,
                username: required("SMTP_USERNAME")?,
                password: required("SMTP_PASSWORD")?,
                from_address: required("SMTP_FROM_ADDRESS")?,
            },
            env,
        })
    }

    /// Ссылка для подтверждения email. Раньше базовый адрес был захардкожен
    /// в `mail/mails.rs` как `http://localhost:3000`.
    pub fn verification_link(&self, token: &str) -> String {
        format!("{}/auth/verify?token={}", self.public_base_url, token)
    }

    /// `Secure` на cookie обязателен в production и мешает при локальной отладке по http.
    pub fn cookie_secure(&self) -> bool {
        self.env.is_production()
    }
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    optional(name).ok_or(ConfigError::Missing(name))
}

fn optional(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => None,
    }
}

fn parse_or<T>(name: &'static str, default: T) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match optional(name) {
        None => Ok(default),
        Some(raw) => raw.parse().map_err(|e: T::Err| ConfigError::Invalid {
            name,
            source: Box::new(e),
        }),
    }
}

#[cfg(test)]
impl Config {
    /// Конфиг для юнит-тестов сервисного слоя — без чтения окружения.
    pub fn for_tests() -> Self {
        Config {
            database_url: "postgres://localhost/test".into(),
            jwt_secret: "test-secret-that-is-long-enough-32b".into(),
            jwt_max_age: Duration::from_secs(3600),
            listen_addr: "127.0.0.1:3000".parse().unwrap(),
            public_base_url: "http://localhost:3000".into(),
            google_web_client_id: "test-client-id".into(),
            smtp: SmtpConfig {
                server: "localhost".into(),
                port: 587,
                username: "test@example.com".into(),
                password: "test".into(),
                from_address: "test@example.com".into(),
            },
            env: Environment::Development,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_link_has_no_double_slash() {
        let mut config = Config::for_tests();
        config.public_base_url = "https://api.example.com".into();
        assert_eq!(
            config.verification_link("abc"),
            "https://api.example.com/auth/verify?token=abc"
        );
    }

    #[test]
    fn jwt_max_age_is_minutes_from_env() {
        let config = Config::for_tests();
        assert_eq!(config.jwt_max_age.as_secs(), 3600);
    }
}
