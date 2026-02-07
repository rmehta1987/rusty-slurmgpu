use once_cell::sync::Lazy;
use regex::Regex;

use crate::constants::{MIN_VALID_JOB_ID, VALID_JOB_STATES};
use crate::errors::GpuReportError;

static USERNAME_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_-]*\$?$").unwrap());
static PARTITION_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]*[a-zA-Z0-9]$|^[a-zA-Z]$").unwrap());

const FORBIDDEN_CHARS: &[char] = &[
    '$', '`', ';', '|', '&', '>', '<', '(', ')', '{', '}', '[', ']', '*', '?', '~', '!', '#',
];

pub struct InputValidator;

impl InputValidator {
    /// Validate and parse comma-separated job IDs.
    pub fn validate_job_ids(job_ids: &str) -> Result<Option<Vec<i64>>, GpuReportError> {
        let trimmed = job_ids.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let mut parsed_ids = Vec::new();
        let mut invalid_ids = Vec::new();

        for part in trimmed.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part.parse::<i64>() {
                Ok(id) if id >= MIN_VALID_JOB_ID => parsed_ids.push(id),
                Ok(_) => invalid_ids.push(format!("{} (must be >= {})", part, MIN_VALID_JOB_ID)),
                Err(_) => invalid_ids.push(format!("{} (not a valid number)", part)),
            }
        }

        if !invalid_ids.is_empty() {
            return Err(GpuReportError::configuration(
                "job IDs",
                &format!("Invalid job IDs: {}", invalid_ids.join(", ")),
                Some("Use comma-separated integers (e.g., '1234,5678')"),
            ));
        }

        if parsed_ids.is_empty() {
            return Err(GpuReportError::configuration(
                "job IDs",
                "No valid job IDs found",
                Some("Provide at least one valid job ID"),
            ));
        }

        Ok(Some(parsed_ids))
    }

    /// Validate and normalize time string for sacct queries.
    pub fn validate_time_string(time_str: &str) -> Result<Option<String>, GpuReportError> {
        let trimmed = time_str.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let lower = trimmed.to_lowercase();

        // Handle special keywords
        match lower.as_str() {
            "yesterday" => {
                let dt = chrono::Local::now() - chrono::Duration::days(1);
                return Ok(Some(dt.format("%Y-%m-%d").to_string()));
            }
            "today" => {
                let dt = chrono::Local::now();
                return Ok(Some(dt.format("%Y-%m-%d").to_string()));
            }
            "now" => {
                let dt = chrono::Local::now();
                return Ok(Some(dt.format("%Y-%m-%dT%H:%M:%S").to_string()));
            }
            _ => {}
        }

        // Try common date formats
        use chrono::NaiveDate;
        use chrono::NaiveDateTime;

        // YYYY-MM-DDTHH:MM:SS
        if let Ok(dt) = NaiveDateTime::parse_from_str(&lower, "%Y-%m-%dT%H:%M:%S") {
            return Ok(Some(dt.format("%Y-%m-%dT%H:%M:%S").to_string()));
        }
        // YYYY-MM-DDTHH:MM
        if let Ok(dt) = NaiveDateTime::parse_from_str(&format!("{}:00", lower), "%Y-%m-%dT%H:%M:%S")
        {
            return Ok(Some(dt.format("%Y-%m-%dT%H:%M:00").to_string()));
        }
        // YYYY-MM-DD
        if let Ok(dt) = NaiveDate::parse_from_str(&lower, "%Y-%m-%d") {
            return Ok(Some(dt.format("%Y-%m-%d").to_string()));
        }
        // MM/DD/YYYY
        if let Ok(dt) = NaiveDate::parse_from_str(&lower, "%m/%d/%Y") {
            return Ok(Some(dt.format("%Y-%m-%d").to_string()));
        }
        // MM/DD/YY
        if let Ok(dt) = NaiveDate::parse_from_str(&lower, "%m/%d/%y") {
            return Ok(Some(dt.format("%Y-%m-%d").to_string()));
        }

        Err(GpuReportError::configuration(
            "time string",
            &format!("Invalid time format: '{}'", trimmed),
            Some("Use formats like: YYYY-MM-DD, YYYY-MM-DDTHH:MM:SS, MM/DD/YYYY, or keywords: yesterday, today, now"),
        ))
    }

    /// Validate and parse comma-separated partition names securely.
    pub fn validate_partition_list(
        partitions: &str,
    ) -> Result<Option<Vec<String>>, GpuReportError> {
        Self::validate_name_list(partitions, "partition names")
    }

    /// Validate and parse comma-separated account names securely.
    pub fn validate_account_list(accounts: &str) -> Result<Option<Vec<String>>, GpuReportError> {
        Self::validate_name_list(accounts, "account names")
    }

    fn validate_name_list(
        input: &str,
        config_type: &str,
    ) -> Result<Option<Vec<String>>, GpuReportError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let mut valid_list = Vec::new();
        let mut invalid_items = Vec::new();

        for name in trimmed.split(',') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }

            // Security checks
            if name.starts_with('-') || name.starts_with('.') {
                invalid_items.push(format!("{} (invalid start character)", name));
                continue;
            }
            if name == ".." || name == "." || name == "/" || name.contains('/') {
                invalid_items.push(format!("{} (reserved or invalid)", name));
                continue;
            }
            if !PARTITION_PATTERN.is_match(name) {
                invalid_items.push(format!("{} (invalid format)", name));
                continue;
            }
            if name.len() > 64 {
                invalid_items.push(format!("{} (too long - max 64 chars)", name));
                continue;
            }
            if name.chars().any(|c| FORBIDDEN_CHARS.contains(&c)) {
                invalid_items.push(format!("{} (contains forbidden characters)", name));
                continue;
            }

            valid_list.push(name.to_string());
        }

        if !invalid_items.is_empty() {
            return Err(GpuReportError::configuration(
                config_type,
                &format!("Invalid {}: {}", config_type, invalid_items.join(", ")),
                Some("Names must start with a letter and contain only alphanumeric, underscore, or hyphen characters"),
            ));
        }

        if valid_list.is_empty() {
            return Err(GpuReportError::configuration(
                config_type,
                &format!("No valid {} found", config_type),
                Some(&format!("Provide at least one valid name")),
            ));
        }

        Ok(Some(valid_list))
    }

    /// Validate username for Slurm queries.
    pub fn validate_user_name(username: &str) -> Result<Option<String>, GpuReportError> {
        let trimmed = username.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        if !USERNAME_PATTERN.is_match(trimmed) {
            return Err(GpuReportError::configuration(
                "username",
                &format!("Invalid username format: '{}'", trimmed),
                Some("Use valid Unix username format (alphanumeric, underscore, hyphen)"),
            ));
        }

        if trimmed.len() > 32 {
            return Err(GpuReportError::configuration(
                "username",
                &format!("Username too long: '{}' (max 32 characters)", trimmed),
                Some("Use a shorter username"),
            ));
        }

        Ok(Some(trimmed.to_string()))
    }

    /// Validate sort field name.
    pub fn validate_sort_field(
        sort_by: &str,
        valid_fields: &[&str],
    ) -> Result<String, GpuReportError> {
        let trimmed = sort_by.trim().to_lowercase();
        if trimmed.is_empty() {
            return Err(GpuReportError::configuration(
                "sort field",
                "Sort field cannot be empty",
                Some(&format!("Use one of: {}", valid_fields.join(", "))),
            ));
        }

        for field in valid_fields {
            if field.to_lowercase() == trimmed {
                return Ok(field.to_string());
            }
        }

        Err(GpuReportError::configuration(
            "sort field",
            &format!("Invalid sort field: '{}'", trimmed),
            Some(&format!("Use one of: {}", valid_fields.join(", "))),
        ))
    }

    /// Validate maximum number of jobs to display.
    pub fn validate_max_jobs(max_jobs: i32) -> Result<i32, GpuReportError> {
        if max_jobs <= 0 {
            return Err(GpuReportError::configuration(
                "max jobs",
                &format!("Max jobs must be positive, got {}", max_jobs),
                Some("Use a positive integer (e.g., 50)"),
            ));
        }
        if max_jobs > 10000 {
            return Err(GpuReportError::configuration(
                "max jobs",
                &format!("Max jobs too large: {} (max 10000)", max_jobs),
                Some("Use a smaller number for better performance"),
            ));
        }
        Ok(max_jobs)
    }

    /// Validate efficiency threshold percentage.
    pub fn validate_efficiency_threshold(threshold: f64) -> Result<f64, GpuReportError> {
        if !(0.0..=100.0).contains(&threshold) {
            return Err(GpuReportError::configuration(
                "efficiency threshold",
                &format!("Threshold must be between 0 and 100, got {}", threshold),
                Some("Use a percentage value between 0 and 100"),
            ));
        }
        Ok(threshold)
    }
}

pub struct SystemValidator;

impl SystemValidator {
    /// Validate job state string.
    pub fn validate_job_state(state: &str) -> Result<Option<String>, GpuReportError> {
        let trimmed = state.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let upper = trimmed.to_uppercase();
        if !VALID_JOB_STATES.contains(upper.as_str()) {
            return Err(GpuReportError::configuration(
                "job state",
                &format!("Invalid job state: '{}'", upper),
                Some(&format!(
                    "Use one of: {}",
                    {
                        let mut states: Vec<_> = VALID_JOB_STATES.iter().collect();
                        states.sort();
                        states
                            .iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                )),
            ));
        }

        Ok(Some(upper))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_job_ids_valid() {
        let result = InputValidator::validate_job_ids("1234,5678").unwrap();
        assert_eq!(result, Some(vec![1234, 5678]));
    }

    #[test]
    fn test_validate_job_ids_empty() {
        let result = InputValidator::validate_job_ids("").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_validate_job_ids_invalid() {
        assert!(InputValidator::validate_job_ids("abc").is_err());
    }

    #[test]
    fn test_validate_user_name_valid() {
        let result = InputValidator::validate_user_name("wjs").unwrap();
        assert_eq!(result, Some("wjs".to_string()));
    }

    #[test]
    fn test_validate_user_name_invalid() {
        assert!(InputValidator::validate_user_name("invalid user!").is_err());
    }

    #[test]
    fn test_validate_partition_list() {
        let result = InputValidator::validate_partition_list("gpu-common,scavenger-gpu").unwrap();
        assert_eq!(
            result,
            Some(vec![
                "gpu-common".to_string(),
                "scavenger-gpu".to_string()
            ])
        );
    }

    #[test]
    fn test_validate_partition_list_security() {
        assert!(InputValidator::validate_partition_list("--inject").is_err());
        assert!(InputValidator::validate_partition_list("../etc").is_err());
        assert!(InputValidator::validate_partition_list("a;b").is_err());
    }

    #[test]
    fn test_validate_time_string_keywords() {
        let result = InputValidator::validate_time_string("today").unwrap();
        assert!(result.is_some());

        let result = InputValidator::validate_time_string("yesterday").unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_validate_time_string_date() {
        let result = InputValidator::validate_time_string("2025-07-24").unwrap();
        assert_eq!(result, Some("2025-07-24".to_string()));
    }

    #[test]
    fn test_validate_sort_field() {
        let fields = &["user", "job_id", "state", "elapsed"];
        assert_eq!(
            InputValidator::validate_sort_field("user", fields).unwrap(),
            "user"
        );
        assert!(InputValidator::validate_sort_field("invalid", fields).is_err());
    }

    #[test]
    fn test_validate_job_state() {
        assert_eq!(
            SystemValidator::validate_job_state("completed").unwrap(),
            Some("COMPLETED".to_string())
        );
        assert!(SystemValidator::validate_job_state("invalid_state").is_err());
    }
}
