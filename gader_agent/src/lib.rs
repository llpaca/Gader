pub mod cert;
pub mod config;
pub mod parsers;
use core::default::Default;
use std::{collections::VecDeque, sync::Mutex};

use gader_common::LogEntry;

/// holds the logs from different containers
///
/// NOTE: Can optimise this later and keep the same API for convenience
#[derive(Debug)]
pub struct AppState {
    pub history: Mutex<VecDeque<LogEntry>>,
    pub secret: String,
    capacity: usize,
}

impl AppState {
    pub fn new(capacity: usize, secret: String) -> Self {
        Self {
            history: Mutex::new(VecDeque::with_capacity(capacity)),
            secret,
            capacity,
        }
    }
    pub fn add_log(&self, entry: LogEntry) {
        let mut state = self.history.lock().unwrap();
        state.push_back(entry);

        if state.len() > self.capacity {
            state.pop_front();
        }
    }

    pub fn get_snapshot(&self) -> Vec<LogEntry> {
        let state = self.history.lock().unwrap();
        state.iter().cloned().collect()
    }
}

impl Default for AppState {
    /// provides a ring buffer with 150 capacity
    fn default() -> Self {
        AppState::new(150, "hello".to_string())
    }
}
