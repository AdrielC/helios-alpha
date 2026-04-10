//! Service traits for swapping file ingest with streaming / broker-backed sources later.
//!
//! v1: file-backed implementations delegate to the existing CSV / JSONL loaders.

use std::path::{Path, PathBuf};

use crate::{
    load_compact_event_shocks_csv, load_compact_region_event_shocks_csv, load_daily_bars_csv,
    load_event_shocks_csv, load_event_shocks_jsonl, DailyBar, EventShock,
};

/// Load a full shock batch (online replay can wrap an iterator around this later).
pub trait EventShockSource {
    fn load_event_shocks(&mut self) -> Result<Vec<EventShock>, String>;
}

/// Load daily bars for execution.
pub trait DailyBarSource {
    fn load_daily_bars(&mut self) -> Result<Vec<DailyBar>, String>;
}

/// File-backed shocks: `csv`, `jsonl`, `compact`, `compact-region` (same as CLI).
#[derive(Debug, Clone)]
pub struct FileEventShockSource {
    pub path: PathBuf,
    pub format: String,
}

impl FileEventShockSource {
    pub fn new(path: impl Into<PathBuf>, format: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            format: format.into(),
        }
    }

    fn read(&self) -> Result<String, String> {
        std::fs::read_to_string(&self.path).map_err(|e| e.to_string())
    }
}

impl EventShockSource for FileEventShockSource {
    fn load_event_shocks(&mut self) -> Result<Vec<EventShock>, String> {
        let raw = self.read()?;
        match self.format.as_str() {
            "jsonl" => load_event_shocks_jsonl(&raw),
            "compact" => load_compact_event_shocks_csv(&raw),
            "compact-region" => load_compact_region_event_shocks_csv(&raw),
            _ => load_event_shocks_csv(&raw),
        }
    }
}

/// File-backed OHLCV bars (CSV).
#[derive(Debug, Clone)]
pub struct FileDailyBarSource {
    pub path: PathBuf,
}

impl FileDailyBarSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl DailyBarSource for FileDailyBarSource {
    fn load_daily_bars(&mut self) -> Result<Vec<DailyBar>, String> {
        let raw = std::fs::read_to_string(&self.path).map_err(|e| e.to_string())?;
        load_daily_bars_csv(&raw)
    }
}

/// Build from paths without storing `PathBuf` (borrowed paths).
pub fn shocks_from_file(path: &Path, format: &str) -> Result<Vec<EventShock>, String> {
    FileEventShockSource::new(path, format).load_event_shocks()
}

pub fn bars_from_file(path: &Path) -> Result<Vec<DailyBar>, String> {
    FileDailyBarSource::new(path).load_daily_bars()
}
