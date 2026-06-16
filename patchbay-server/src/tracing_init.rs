use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{
    Layer, Registry,
    filter::{LevelFilter, ParseError, Targets},
    fmt::format::{DefaultFields, Format, Full},
    prelude::__tracing_subscriber_SubscriberExt,
};

type BoxedLayer = Box<dyn Layer<Registry> + Send + Sync + 'static>;
type StdoutWriter = fn() -> std::io::Stdout;

const PATCHBAY_LOG_ENV: &str = "PATCHBAY_LOG";
const PATCHBAY_SQLX_LOG_ENV: &str = "PATCHBAY_SQLX_LOG";
const SQLX_TARGET: &str = "sqlx";

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FmtLayerMode {
    Standard,
    #[default]
    Pretty,
    Json,
}

#[derive(Debug, Clone, Copy)]
pub struct TracingConfig {
    pub with_target: bool,
    pub with_file: bool,
    pub with_line_number: bool,
    pub with_ansi_coloring: bool,
    pub with_thread_name: bool,
    pub with_thread_id: bool,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            with_target: true,
            with_file: true,
            with_line_number: true,
            with_ansi_coloring: true,
            with_thread_name: false,
            with_thread_id: false,
        }
    }
}

impl TracingConfig {
    fn into_fmt_layer(
        self,
    ) -> tracing_subscriber::fmt::Layer<Registry, DefaultFields, Format<Full>, StdoutWriter> {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout as StdoutWriter)
            .with_target(self.with_target)
            .with_file(self.with_file)
            .with_line_number(self.with_line_number)
            .with_ansi(self.with_ansi_coloring)
            .with_thread_names(self.with_thread_name)
            .with_thread_ids(self.with_thread_id)
    }
}

fn default_fmt_filter(default_log_level: LevelFilter) -> Targets {
    Targets::new()
        .with_default(default_log_level)
        .with_target("tokio", LevelFilter::WARN)
        .with_target("runtime", LevelFilter::WARN)
        .with_target(SQLX_TARGET, LevelFilter::WARN)
}

fn parse_fmt_filter(value: &str) -> Result<Targets, ParseError> {
    value.trim().parse()
}

fn build_fmt_filter(
    default_log_level: LevelFilter,
    configured_filter: Option<&str>,
    sqlx_log_level: Option<LevelFilter>,
) -> Result<Targets, ParseError> {
    let mut filter = match configured_filter
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => parse_fmt_filter(value)?,
        None => default_fmt_filter(default_log_level),
    };

    if let Some(level) = sqlx_log_level {
        filter = filter.with_target(SQLX_TARGET, level);
    }

    Ok(filter)
}

fn read_env_var(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => None,
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(err) => {
            eprintln!("Ignoring unreadable {name}: {err}");
            None
        }
    }
}

fn read_sqlx_log_level() -> Option<LevelFilter> {
    let value = read_env_var(PATCHBAY_SQLX_LOG_ENV)?;
    match value.trim().parse::<LevelFilter>() {
        Ok(level) => Some(level),
        Err(err) => {
            eprintln!("Ignoring invalid {PATCHBAY_SQLX_LOG_ENV} value {value:?}: {err}");
            None
        }
    }
}

fn build_fmt_filter_from_env(default_log_level: LevelFilter) -> Targets {
    let configured_filter = read_env_var(PATCHBAY_LOG_ENV);
    let sqlx_log_level = read_sqlx_log_level();

    match build_fmt_filter(
        default_log_level,
        configured_filter.as_deref(),
        sqlx_log_level,
    ) {
        Ok(filter) => filter,
        Err(err) => {
            if let Some(value) = configured_filter {
                eprintln!("Ignoring invalid {PATCHBAY_LOG_ENV} value {value:?}: {err}");
            }

            build_fmt_filter(default_log_level, None, sqlx_log_level)
                .expect("default tracing filter must be valid")
        }
    }
}

fn build_fmt_layer(mode: FmtLayerMode, config: TracingConfig) -> BoxedLayer {
    let fmt_layer = config.into_fmt_layer();
    match mode {
        FmtLayerMode::Standard => Box::new(fmt_layer),
        FmtLayerMode::Pretty => Box::new(fmt_layer.pretty()),
        FmtLayerMode::Json => Box::new(fmt_layer.json()),
    }
}

pub fn init() {
    let fmt_filter = build_fmt_filter_from_env(LevelFilter::INFO);
    let fmt_layer = build_fmt_layer(FmtLayerMode::Pretty, Default::default());
    let fmt_layer_filtered = fmt_layer.with_filter(fmt_filter);

    Registry::default().with(fmt_layer_filtered).init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;

    #[test]
    fn default_filter_hides_sqlx_info() {
        let filter = default_fmt_filter(LevelFilter::INFO);

        assert!(filter.would_enable("patchbay_server", &Level::INFO));
        assert!(!filter.would_enable("sqlx::query", &Level::INFO));
        assert!(filter.would_enable("sqlx::query", &Level::WARN));
    }

    #[test]
    fn patchbay_log_filter_can_enable_sqlx_info() {
        let filter = build_fmt_filter(LevelFilter::INFO, Some("info,sqlx=info"), None).unwrap();

        assert!(filter.would_enable("sqlx::query", &Level::INFO));
    }

    #[test]
    fn sqlx_log_level_can_override_default_target() {
        let filter = build_fmt_filter(LevelFilter::INFO, None, Some(LevelFilter::DEBUG)).unwrap();

        assert!(filter.would_enable("sqlx::query", &Level::DEBUG));
        assert!(!filter.would_enable("sqlx::query", &Level::TRACE));
    }

    #[test]
    fn configured_filter_replaces_default_filter() {
        let filter = build_fmt_filter(LevelFilter::INFO, Some("off"), None).unwrap();

        assert!(!filter.would_enable("patchbay_server", &Level::ERROR));
        assert!(!filter.would_enable("sqlx::query", &Level::WARN));
    }
}
