use core::convert::Into;

use regex::Regex;

use super::{LogEntry, LogParser};

pub struct ImmichParser {
    ansi_re: Regex,
    log_re: Regex,
    last_timestamp: std::cell::RefCell<String>,
}

impl ImmichParser {
    pub fn new() -> Self {
        Self {
            ansi_re: Regex::new(r"\x1b\[[0-9;]*m").expect("Invalid ANSI regex"),
            log_re: Regex::new(r"\[Nest\]\s+\d+\s+-\s+(?P<time>\d{2}/\d{2}/\d{4},\s+\d{1,2}:\d{2}:\d{2}\s+[AP]M)\s+(?P<level>[A-Z]+)\s+\[(?P<context>[^\]]+)\]\s+(?P<msg>.+)").expect("Invalid regex"),
            last_timestamp: std::cell::RefCell::new("Unknown".to_string()),
        }
    }

    pub fn strip_ansi(&self, s: &str) -> String {
        self.ansi_re.replace_all(s, "").to_string()
    }
}

impl LogParser for ImmichParser {
    fn parse(&self, line: &str) -> Option<LogEntry> {
        let clean_line = self.strip_ansi(line);

        if let Some(caps) = self.log_re.captures(&clean_line) {
            let ts = &caps["time"];

            *self.last_timestamp.borrow_mut() = ts.to_string();

            return Some(LogEntry {
                service: "immich".into(),
                timestamp: ts.into(),
                level: caps["level"].into(),
                context: caps["context"].into(),
                message: caps["msg"].into(),
            });
        }

        // if it is a stacktrace
        if !clean_line.trim().is_empty() {
            return Some(LogEntry {
                service: "immich".into(),
                timestamp: self.last_timestamp.borrow().clone().into(),
                level: "RAW".into(),
                context: "Trace".into(),
                message: clean_line.into(),
            });
        }

        None
    }
}

impl Default for ImmichParser {
    fn default() -> Self {
        Self::new()
    }
}
