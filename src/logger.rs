use std::error::Error;
use std::fs::OpenOptions;
use std::path::Path;

use slog::Drain;
use slog::FnValue;
use slog::Level;
use slog::PushFnValue;
use slog::Record;
use slog_scope::GlobalLoggerGuard;

pub fn init(log_path: impl AsRef<Path>) -> Result<GlobalLoggerGuard, Box<dyn Error>> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    let log_level = match std::env::var("FZFW_LOG_LEVEL")
        .unwrap_or_else(|_| "info".to_string())
        .to_lowercase()
        .as_str()
    {
        "trace" => Level::Trace,
        "debug" => Level::Debug,
        "info" => Level::Info,
        "warning" => Level::Warning,
        "error" => Level::Error,
        "critical" => Level::Critical,
        _ => Level::Info,
    };

    let drain = slog_json::Json::new(file)
        .add_key_value(o!(
            "level"   => FnValue(move |r : &Record| {
                r.level().as_str()
            }),
            "time" => FnValue(move |_ : &Record| {
                chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            }),
            "loc" => FnValue(move |r : &Record| {
                format!("{}:{}:{}", r.file(), r.line(), r.column())
            }),
            "message" => PushFnValue(move |record : &Record, ser| {
                ser.emit(record.msg())
            }),
        ))
        .build()
        .filter(move |record| record.level().is_at_least(log_level))
        .fuse();

    let drain = slog_async::Async::new(drain).build().fuse();
    let log = slog::Logger::root(drain, o!());
    let guard = slog_scope::set_global_logger(log);
    Ok(guard)
}

// Added a wrapper type to make using serde values easier
// cf. https://github.com/slog-rs/slog/commit/c1989580e782cf5547e2f74240fd24dd91bf7fd3

#[derive(Clone, Debug, serde::Serialize)]
pub struct Serde<T>(pub T)
where
    T: serde::Serialize + Clone + Send + 'static;

impl<T> slog::SerdeValue for Serde<T>
where
    T: serde::Serialize + Clone + Send + 'static,
{
    fn as_serde(&self) -> &dyn erased_serde::Serialize {
        &self.0
    }

    fn to_sendable(&self) -> Box<dyn slog::SerdeValue + Send + 'static> {
        Box::new(self.clone())
    }
}

impl<T> slog::Value for Serde<T>
where
    T: serde::Serialize + Clone + Send + 'static,
{
    fn serialize(
        &self,
        _: &slog::Record<'_>,
        key: slog::Key,
        serializer: &mut dyn slog::Serializer,
    ) -> slog::Result {
        serializer.emit_serde(key, self)
    }
}
