use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;

// GPU Efficiency Thresholds
pub const GPU_IDLE_THRESHOLD_PERCENT: f64 = 50.0;
pub const GPU_MEMORY_IDLE_THRESHOLD_PERCENT: f64 = 50.0;
pub const MAX_EFFICIENCY_PERCENT: f64 = 100.0;

// Time Calculations
pub const SECONDS_PER_MINUTE: i64 = 60;
pub const SECONDS_PER_HOUR: i64 = 3600;
pub const SECONDS_PER_DAY: i64 = 86400;

// Default Time Limits (in seconds)
pub const DEFAULT_PARTITION_TIME_LIMIT_DAYS: i64 = 4;
pub const DEFAULT_PARTITION_TIME_LIMIT_SECONDS: i64 =
    DEFAULT_PARTITION_TIME_LIMIT_DAYS * SECONDS_PER_DAY;

// Memory Unit Conversions
pub const BYTES_PER_MB: f64 = 1_048_576.0; // 1024 * 1024
pub const MB_PER_GB: f64 = 1024.0;

// GPU Utilization Format Detection
pub const GPU_NANOSECOND_THRESHOLD: f64 = 1_000_000.0;
pub const GPU_NANOSECOND_TO_PERCENT_DIVISOR: f64 = 10_000_000.0;

// String Truncation Limits
pub const MAX_ERROR_MESSAGE_LENGTH: usize = 500;
pub const MAX_SACCT_OUTPUT_PREVIEW: usize = 500;

// Validation Constants
pub const MIN_VALID_JOB_ID: i64 = 1;
pub const MAX_REASONABLE_GPU_COUNT: i64 = 100;
pub const MAX_REASONABLE_CPU_COUNT: i64 = 1000;
pub const MAX_REASONABLE_MEMORY_GB: f64 = 10000.0;

// Output Formatting
pub const PERCENTAGE_DECIMAL_PLACES: usize = 1;
pub const TIME_FORMAT_PADDING: usize = 2;

// State Filtering
pub static VALID_JOB_STATES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "PENDING",
        "RUNNING",
        "COMPLETED",
        "FAILED",
        "CANCELLED",
        "TIMEOUT",
        "NODE_FAIL",
        "PREEMPTED",
    ]
    .into_iter()
    .collect()
});

// Time parsing patterns
pub static TIME_WITH_DAYS_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\d+)-(\d+):(\d+):(\d+)").unwrap());
pub static TIME_HMS_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\d+):(\d+):(\d+)").unwrap());
pub static TIME_HM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(\d+):(\d+)$").unwrap());

// Efficiency Display Thresholds
pub const EXCELLENT_EFFICIENCY_THRESHOLD: f64 = 80.0;
pub const GOOD_EFFICIENCY_THRESHOLD: f64 = 60.0;
pub const POOR_EFFICIENCY_THRESHOLD: f64 = 30.0;

// Color coding for efficiency display
pub struct EfficiencyColors;

impl EfficiencyColors {
    pub const EXCELLENT: &'static str = "green";
    pub const GOOD: &'static str = "yellow";
    pub const FAIR: &'static str = "bright red"; // colored crate doesn't have "orange"
    pub const POOR: &'static str = "red";
    pub const UNKNOWN: &'static str = "white";
}

// Color mapping for job states
pub struct StateColors;

impl StateColors {
    pub fn get(state: &str) -> &'static str {
        match state.to_uppercase().as_str() {
            "COMPLETED" => "green",
            "FAILED" => "red",
            "CANCELLED" => "yellow",
            "TIMEOUT" => "yellow",
            "PENDING" => "blue",
            "RUNNING" => "cyan",
            _ => "white",
        }
    }
}
