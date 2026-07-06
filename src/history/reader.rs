//! Read filtered history entries.

use super::writer::HistoryEntry;
use crate::config::paths::get_history_path;

#[derive(Debug, Clone, Default)]
pub struct LoadHistoryOptions {
    pub limit: Option<usize>,
    pub search: Option<String>,
}

pub fn load_history(options: &LoadHistoryOptions) -> Vec<HistoryEntry> {
    let path = get_history_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries: Vec<HistoryEntry> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    if let Some(search) = &options.search {
        let term = search.to_lowercase();
        entries.retain(|e| e.message_preview.to_lowercase().contains(&term));
    }
    if let Some(limit) = options.limit {
        if entries.len() > limit {
            entries.drain(..entries.len() - limit);
        }
    }
    entries
}
