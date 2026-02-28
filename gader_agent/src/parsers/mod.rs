use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};

pub mod immich;
pub mod vaultwarden;

#[derive(Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub service: String,
    pub timestamp: String,
    pub level: String,
    pub context: String,
    pub message: String,
}

pub trait LogParser {
    /// Parses the logs specific to a service and returns them as `LogEntry`
    fn parse(&self, line: &str) -> Option<LogEntry>;
}

impl Display for LogEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Format: [2024-02-28 12:00] [IMMICH] [INFO] (Context) Message
        write!(
            f,
            "[{}] [{}] [{}] ({}) {}",
            self.timestamp,
            self.service.to_uppercase(),
            self.level,
            self.context,
            self.message
        )
    }
}
