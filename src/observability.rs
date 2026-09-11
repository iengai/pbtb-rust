//! The tracing subscriber every binary installs, and the Sentry client behind
//! it when a DSN is configured.
//!
//! Log lines go to stderr in every process: docker and the Lambda runtime
//! capture it like stdout, and `mcp_stdio` owns stdout for the protocol
//! stream. Events at `ERROR` also become Sentry events, `WARN` and `INFO`
//! become breadcrumbs on the next event, and a `tags.<name>` field on an
//! event becomes a searchable Sentry tag. Without `APP__SENTRY__DSN` nothing
//! leaves the process.

use std::time::Duration;

use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

const DSN: &str = "APP__SENTRY__DSN";
const ENVIRONMENT: &str = "APP__SENTRY__ENVIRONMENT";

/// Owns the Sentry client for the life of the process. Dropping it flushes
/// what is still queued, which is the last chance a process gets on shutdown.
pub struct Telemetry {
    guard: Option<sentry::ClientInitGuard>,
}

impl Telemetry {
    /// Install the subscriber and, when a DSN is set, the Sentry client.
    ///
    /// Reads the environment directly rather than the typed config: this runs
    /// before `load_config`, so a config error is itself reported. `component`
    /// tags every event with the binary that sent it; all four share one
    /// Sentry project.
    pub fn init(component: &'static str) -> Self {
        let dsn = std::env::var(DSN).ok().filter(|s| !s.trim().is_empty());
        let guard = dsn.map(|dsn| {
            let mut options = sentry::ClientOptions::new();
            options.release = Some(concat!("pbtb-rust@", env!("CARGO_PKG_VERSION")).into());
            options.environment = std::env::var(ENVIRONMENT)
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(Into::into);
            options.attach_stacktrace = true;
            sentry::init((dsn, options))
        });
        sentry::configure_scope(|scope| scope.set_tag("component", component));

        tracing_subscriber::registry()
            .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_ansi(false),
            )
            .with(sentry::integrations::tracing::layer())
            .init();

        Self { guard }
    }

    /// Push queued events out now.
    ///
    /// A Lambda process is frozen between invocations, so an event still in
    /// the transport's queue when the handler returns may never leave; call
    /// this before returning from each invocation.
    pub fn flush(&self) {
        if let Some(client) = &self.guard {
            client.flush(Some(Duration::from_secs(2)));
        }
    }
}
