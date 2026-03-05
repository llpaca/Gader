use core::convert::Into;

use regex::Regex;

use super::{LogEntry, LogParser};

pub struct VWParser {
    ansi_re: Regex,
    log_re: Regex,
}

impl VWParser {
    pub fn new() -> Self {
        Self {
            ansi_re: Regex::new(r"\x1b\[[0-9;]*m").expect("Invalid ANSI regex"),
            log_re: Regex::new(
                r"^\[(?P<time>[^\]]+)\]\[(?P<context>[^\]]+)\]\[(?P<level>[^\]]+)\]\s+(?P<msg>.+)",
            )
            .expect("Invalid Vaultwarden regex"),
        }
    }

    pub fn strip_ansi(&self, s: &str) -> String {
        self.ansi_re.replace_all(s, "").to_string()
    }
}

impl LogParser for VWParser {
    fn parse(&self, line: &str) -> Option<LogEntry> {
        let clean_line = self.strip_ansi(line);

        if let Some(caps) = self.log_re.captures(&clean_line) {
            return Some(LogEntry {
                service: "vaultwarden".into(),
                timestamp: caps["time"].into(),
                level: caps["level"].into(),
                context: caps["context"].into(),
                message: caps["msg"].into(),
            });
        }

        if !clean_line.trim().is_empty() {
            return Some(LogEntry {
                service: "vaultwarden".into(),
                timestamp: "Unknown".into(),
                level: "RAW".into(),
                context: "General".into(),
                message: clean_line.into(),
            });
        }

        None
    }
}

impl Default for VWParser {
    fn default() -> Self {
        Self::new()
    }
}
