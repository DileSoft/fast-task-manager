//! Сохранение настроек между запусками: `%APPDATA%\fast-task-manager\settings.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub columns: Vec<String>,
    pub sort: String,
    pub sort_desc: bool,
    pub group_by_type: bool,
    pub interval_ms: u64,
    pub paused: bool,
    /// `None` — следовать теме Windows, иначе явный выбор пользователя.
    pub dark: Option<bool>,
    pub perf_kind: String,
    pub perf_index: usize,
    pub window_w: f32,
    pub window_h: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            columns: vec![
                "name".to_string(),
                "status".to_string(),
                "cpu".to_string(),
                "memory".to_string(),
                "disk".to_string(),
                "network".to_string(),
                "gpu".to_string(),
            ],
            sort: "cpu".to_string(),
            sort_desc: true,
            group_by_type: false,
            interval_ms: 1000,
            paused: false,
            dark: None,
            perf_kind: "cpu".to_string(),
            perf_index: 0,
            window_w: 900.0,
            window_h: 570.0,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let Some(path) = path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let text = text.trim_start_matches('\u{feff}');
                serde_json::from_str(text).unwrap_or_default()
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

fn path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let mut path = PathBuf::from(appdata);
    path.push("fast-task-manager");
    path.push("settings.json");
    Some(path)
}
