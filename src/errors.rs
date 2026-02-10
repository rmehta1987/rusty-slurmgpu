use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub(crate) enum GpuReportError {
    #[error("Slurm command '{command}' failed with exit code {exit_code}")]
    SlurmCommand {
        command: String,
        exit_code: i32,
        stderr: String,
        stdout_preview: String,
    },

    #[error("{message}")]
    JobDataParsing {
        message: String,
        job_id: Option<String>,
        field: Option<String>,
        raw_data_preview: Option<String>,
    },

    #[error("Configuration error in {config_type}: {issue}")]
    Configuration {
        config_type: String,
        issue: String,
        suggestion: Option<String>,
    },
}

#[allow(dead_code)]
impl GpuReportError {
    pub fn slurm_command(command: &str, exit_code: i32, stderr: &str, stdout: &str) -> Self {
        Self::SlurmCommand {
            command: command.to_string(),
            exit_code,
            stderr: stderr.chars().take(500).collect(),
            stdout_preview: stdout.chars().take(200).collect(),
        }
    }

    pub fn job_parsing(message: &str) -> Self {
        Self::JobDataParsing {
            message: message.to_string(),
            job_id: None,
            field: None,
            raw_data_preview: None,
        }
    }

    pub fn job_parsing_with_context(job_id: &str, field: &str, raw_data: &str) -> Self {
        Self::JobDataParsing {
            message: format!("Failed to parse field '{}' for job {}", field, job_id),
            job_id: Some(job_id.to_string()),
            field: Some(field.to_string()),
            raw_data_preview: Some(raw_data.chars().take(100).collect()),
        }
    }

    pub fn configuration(config_type: &str, issue: &str, suggestion: Option<&str>) -> Self {
        Self::Configuration {
            config_type: config_type.to_string(),
            issue: issue.to_string(),
            suggestion: suggestion.map(|s| s.to_string()),
        }
    }

    pub fn details(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        match self {
            Self::SlurmCommand {
                command,
                exit_code,
                stderr,
                stdout_preview,
            } => {
                map.insert("command".to_string(), command.clone());
                map.insert("exit_code".to_string(), exit_code.to_string());
                map.insert("stderr".to_string(), stderr.clone());
                map.insert("stdout_preview".to_string(), stdout_preview.clone());
            }
            Self::JobDataParsing {
                job_id,
                field,
                raw_data_preview,
                ..
            } => {
                if let Some(id) = job_id {
                    map.insert("job_id".to_string(), id.clone());
                }
                if let Some(f) = field {
                    map.insert("field".to_string(), f.clone());
                }
                if let Some(rd) = raw_data_preview {
                    map.insert("raw_data_preview".to_string(), rd.clone());
                }
            }
            Self::Configuration {
                config_type,
                issue,
                suggestion,
            } => {
                map.insert("config_type".to_string(), config_type.clone());
                map.insert("issue".to_string(), issue.clone());
                if let Some(s) = suggestion {
                    map.insert("suggestion".to_string(), s.clone());
                }
            }
        }
        map
    }

    pub fn suggestion(&self) -> Option<&str> {
        match self {
            Self::Configuration { suggestion, .. } => suggestion.as_deref(),
            _ => None,
        }
    }
}
