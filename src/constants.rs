// Missing Value Placeholder
pub(crate) const NO_DATA: &str = "---";

// GPU Efficiency Thresholds
pub(crate) const GPU_IDLE_THRESHOLD_PERCENT: f64 = 50.0;
pub(crate) const GPU_MEMORY_IDLE_THRESHOLD_PERCENT: f64 = 50.0;
pub(crate) const MAX_EFFICIENCY_PERCENT: f64 = 100.0;

// Time Calculations
pub(crate) const SECONDS_PER_MINUTE: i64 = 60;
pub(crate) const SECONDS_PER_HOUR: i64 = 3600;

// Default Time Limits (in seconds)
pub(crate) const DEFAULT_PARTITION_TIME_LIMIT_SECONDS: i64 = 4 * 86400; // 4 days

// Memory Unit Conversions
pub(crate) const BYTES_PER_MB: f64 = 1_048_576.0; // 1024 * 1024
pub(crate) const MB_PER_GB: f64 = 1024.0;

// GPU Utilization Format Detection
pub(crate) const GPU_NANOSECOND_THRESHOLD: f64 = 1_000_000.0;
pub(crate) const GPU_NANOSECOND_TO_PERCENT_DIVISOR: f64 = 10_000_000.0;

// Validation Constants
pub(crate) const MIN_VALID_JOB_ID: i64 = 1;

// Efficiency Display Thresholds
pub(crate) const EXCELLENT_EFFICIENCY_THRESHOLD: f64 = 80.0;
pub(crate) const GOOD_EFFICIENCY_THRESHOLD: f64 = 60.0;
pub(crate) const POOR_EFFICIENCY_THRESHOLD: f64 = 30.0;
