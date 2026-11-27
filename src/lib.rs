pub mod cache;
pub mod client;
pub mod format;
pub mod http;
pub mod model;
pub mod parser;

use chrono::{SecondsFormat, Utc};

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct CanyonError {
    pub code: &'static str,
    pub message: String,
}

impl CanyonError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, CanyonError>;

pub const DEFAULT_LOCALE: &str = "en-de";

pub fn normalize_locale(input: &str) -> Result<String> {
    let locale = input.trim().replace('_', "-").to_ascii_lowercase();
    if locale.len() == 5
        && locale.split_once('-').is_some_and(|(a, b)| {
            a.len() == 2
                && b.len() == 2
                && a.bytes().chain(b.bytes()).all(|c| c.is_ascii_lowercase())
        })
    {
        return Ok(locale);
    }
    Err(CanyonError::new(
        "INVALID_LOCALE",
        format!(
            "Invalid locale '{input}'. Use Canyon's language-country form, for example en-de or de-de."
        ),
    ))
}

pub fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
