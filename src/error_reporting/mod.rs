//! Error and panic reporting over the Sentry protocol, to GlitchTip, Hawk or
//! Sentry. Enabled by `SENTRY_DSN`; every `tracing::error!` and every panic
//! becomes an event. Field values whose names look sensitive (email, token,
//! password, ...) are replaced; messages are sent as they are, so they must
//! not contain personal data.

mod backtrace;
mod dsn;
mod event;
mod layer;
mod transport;

pub use dsn::{Dsn, DsnError};
pub use layer::ErrorReportingLayer;
pub use transport::Transport;

use event::EventDefaults;
use layer::PANIC_TARGET;
use std::backtrace::Backtrace;
use std::panic::PanicHookInfo;
use std::time::Duration;

const CLIENT_NAME: &str = concat!("pills-server/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("SENTRY_DSN is invalid: {0}")]
    Dsn(#[from] DsnError),
    #[error("could not start the error reporting thread: {0}")]
    Thread(#[from] std::io::Error),
}

/// Keeps the transport to flush queued events on shutdown.
pub struct ErrorReporting {
    transport: Transport,
}

impl ErrorReporting {
    /// Reads `SENTRY_DSN`, `SENTRY_ENVIRONMENT` (default: `APP_ENV`, then
    /// `development`) and `SENTRY_RELEASE` (default: crate name and version).
    /// Returns `None` when `SENTRY_DSN` is not set.
    pub fn from_env() -> Result<Option<(Self, ErrorReportingLayer)>, InitError> {
        let Some(raw_dsn) = env("SENTRY_DSN") else {
            return Ok(None);
        };
        let dsn = Dsn::parse(&raw_dsn)?;
        let defaults = EventDefaults {
            release: env("SENTRY_RELEASE").unwrap_or_else(|| {
                format!("{}@{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
            }),
            environment: env("SENTRY_ENVIRONMENT")
                .or_else(|| env("APP_ENV"))
                .unwrap_or_else(|| "development".to_string()),
            server_name: env("HOSTNAME"),
        };

        let transport = Transport::start(dsn, CLIENT_NAME.to_string())?;
        let layer = ErrorReportingLayer::new(transport.clone(), defaults);
        Ok(Some((Self { transport }, layer)))
    }

    pub fn flush(&self, timeout: Duration) -> bool {
        self.transport.flush(timeout)
    }
}

/// Routes panics through `tracing`: the panic lands in the logs and, with
/// reporting enabled, is sent before the hook returns.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(panic_hook));
}

fn panic_hook(info: &PanicHookInfo<'_>) {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|text| text.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic with a non-string payload".to_string());
    let location = info
        .location()
        .map(|location| location.to_string())
        .unwrap_or_default();
    let thread = std::thread::current();
    let backtrace = Backtrace::force_capture();

    tracing::error!(
        target: PANIC_TARGET,
        thread = thread.name().unwrap_or("unnamed"),
        location = %location,
        backtrace = %backtrace,
        "{message}"
    );
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
