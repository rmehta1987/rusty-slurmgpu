pub mod app;
pub mod data;
pub mod input;
pub mod report_tab;
pub mod stat_tab;
pub mod usage_tab;
pub mod widgets;

use std::io;

#[derive(Debug, Clone)]
pub struct TuiConfig {
    pub partition: Option<String>,
    pub partitions: Option<Vec<String>>,
    pub user: Option<String>,
    pub effective_user: Option<String>,
    pub starttime: Option<String>,
    pub refresh_interval: u64,
}

pub fn run_tui(config: TuiConfig) -> io::Result<()> {
    app::run_app(config)
}
