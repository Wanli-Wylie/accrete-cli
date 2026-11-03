use serde_json::{Value, json};
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    pub kind: String,
    pub message: String,
    pub details: Value,
    pub exit_code: i32,
}

impl Error {
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        let exit_code = match kind {
            "invalid" | "invalid_input" | "unsupported_format" | "format" => 2,
            "config" | "project" | "not_found" => 3,
            "conflict" => 4,
            "dependency" => 5,
            "external" | "incomplete" | "recovery" => 7,
            "durability" => 8,
            _ => 6,
        };
        Self {
            kind: kind.into(),
            message: message.into(),
            details: Value::Null,
            exit_code,
        }
    }
    pub fn detail(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
    pub fn json(&self) -> Value {
        json!({"ok": false, "error": {"kind":self.kind,"message":self.message,"details":self.details},"exit_code":self.exit_code})
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new("storage", e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::new("invalid", e.to_string())
    }
}
impl From<toml::de::Error> for Error {
    fn from(e: toml::de::Error) -> Self {
        Self::new("config", e.to_string())
    }
}
impl From<toml::ser::Error> for Error {
    fn from(e: toml::ser::Error) -> Self {
        Self::new("config", e.to_string())
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("storage", e.to_string())
    }
}
impl From<serde_yaml::Error> for Error {
    fn from(e: serde_yaml::Error) -> Self {
        Self::new("invalid", e.to_string())
    }
}
impl From<walkdir::Error> for Error {
    fn from(e: walkdir::Error) -> Self {
        Self::new("storage", e.to_string())
    }
}
