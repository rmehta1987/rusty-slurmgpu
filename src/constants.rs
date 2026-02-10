use once_cell::sync::Lazy;
use std::collections::HashSet;

// GPU Efficiency Thresholds
pub const GPU_IDLE_THRESHOLD_PERCENT: f64 = 50.0;
pub const GPU_MEMORY_IDLE_THRESHOLD_PERCENT: f64 = 50.0;
pub const MAX_EFFICIENCY_PERCENT: f64 = 100.0;

// Time Calculations
pub const SECONDS_PER_MINUTE: i64 = 60;
pub const SECONDS_PER_HOUR: i64 = 3600;

// Default Time Limits (in seconds)
pub const DEFAULT_PARTITION_TIME_LIMIT_SECONDS: i64 = 4 * 86400; // 4 days

// Memory Unit Conversions
pub const BYTES_PER_MB: f64 = 1_048_576.0; // 1024 * 1024
pub const MB_PER_GB: f64 = 1024.0;

// GPU Utilization Format Detection
pub const GPU_NANOSECOND_THRESHOLD: f64 = 1_000_000.0;
pub const GPU_NANOSECOND_TO_PERCENT_DIVISOR: f64 = 10_000_000.0;

// Validation Constants
pub const MIN_VALID_JOB_ID: i64 = 1;

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

// Efficiency Display Thresholds
pub const EXCELLENT_EFFICIENCY_THRESHOLD: f64 = 80.0;
pub const GOOD_EFFICIENCY_THRESHOLD: f64 = 60.0;
pub const POOR_EFFICIENCY_THRESHOLD: f64 = 30.0;
