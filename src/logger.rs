#![allow(dead_code)]

use log::LevelFilter;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use serde::Serialize;
use serde_json::json;
use std::error::Error;

pub fn init() -> Result<(), Box<dyn Error>> {
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "\\{\"level\":\"{level}\",\"body\":{message}\\}{n}",
        )))
        .build("/tmp/myfzf-rs.log")?;
    let config = Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(Root::builder().appender("logfile").build(LevelFilter::Info))?;
    log4rs::init_config(config)?;
    Ok(())
}

pub fn error(context: impl Into<String>, message: impl Serialize) {
    log::error!("{}", LogItem::new(context, message).to_string());
}

pub fn warn(context: impl Into<String>, message: impl Serialize) {
    log::warn!("{}", LogItem::new(context, message).to_string());
}

pub fn info(context: impl Into<String>, message: impl Serialize) {
    log::info!("{}", LogItem::new(context, message).to_string());
}

pub fn debug(context: impl Into<String>, message: impl Serialize) {
    log::debug!("{}", LogItem::new(context, message).to_string());
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Internal
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize)]
struct LogItem {
    context: String,
    message: serde_json::Value,
}

impl LogItem {
    fn new(context: impl Into<String>, message: impl Serialize) -> Self {
        Self {
            context: context.into(),
            message: json!(message),
        }
    }
    fn to_string(&self) -> String {
        serde_json::to_string(&self).expect("Failed to serialize log item")
    }
}
