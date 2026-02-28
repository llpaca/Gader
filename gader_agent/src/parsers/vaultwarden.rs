use regex::Regex;

use super::{LogParser, LogEntry};

pub struct VWParser {
    ansi_re: Regex,
    log_re: Regex,
}

impl VWParser {
    pub fn new() -> Self {
        Self {
            ansi_re: Regex::new(r"\x1b\[[0-9;]*m").expect("Invalid ANSI regex"),
            log_re: Regex::new(r"^\[(?P<time>[^\]]+)\]\[(?P<context>[^\]]+)\]\[(?P<level>[^\]]+)\]\s+(?P<msg>.+)").expect("Invalid Vaultwarden regex"),
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
                service: "vaultwarden".to_string(),
                timestamp: caps["time"].to_string(),
                level: caps["level"].to_string(),
                context: caps["context"].to_string(),
                message: caps["msg"].to_string(),
            });
        }

        if !clean_line.trim().is_empty() {
            return Some(LogEntry {
                service: "vaultwarden".to_string(),
                timestamp: "Unknown".to_string(),
                level: "RAW".to_string(),
                context: "General".to_string(),
                message: clean_line,
            });
        }

        None
    }
}
