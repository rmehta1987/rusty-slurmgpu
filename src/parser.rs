use std::collections::HashMap;
use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::*;
use crate::tres_parser::TresParser;
use crate::tres_registry::TresRegistry;

static TRES_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([^=,]+)=([^,]+)").unwrap());
static MEMORY_UNIT_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(\d+(?:\.\d+)?)([KMGT]?)$").unwrap());

pub struct SlurmJobParser;

impl SlurmJobParser {
    /// Parse sacct parseable format output from a file.
    pub fn parse_file(file_path: &Path) -> Result<Vec<SlurmJob>, String> {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read file {}: {}", file_path.display(), e))?;
        Self::parse_string(&content)
    }

    /// Parse sacct parseable format output from a string.
    pub fn parse_string(content: &str) -> Result<Vec<SlurmJob>, String> {
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() {
            return Ok(Vec::new());
        }

        // First line is the header
        let headers = Self::parse_header(lines[0]);
        if headers.is_empty() {
            return Ok(Vec::new());
        }

        // Parse data lines
        let mut raw_jobs = Vec::new();
        for (line_num, line) in lines[1..].iter().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match Self::parse_data_line(line, &headers) {
                Some(job_data) => raw_jobs.push(job_data),
                None => {
                    eprintln!("Warning: Failed to parse line {}", line_num + 2);
                }
            }
        }

        // Group jobs and steps
        Ok(Self::group_jobs_and_steps(raw_jobs))
    }

    fn parse_header(header_line: &str) -> Vec<String> {
        let mut line = header_line.trim_end();
        // Remove trailing delimiter
        if line.ends_with('|') || line.ends_with('\t') {
            line = &line[..line.len() - 1];
        }

        let delimiter = if line.contains('\t') { '\t' } else { '|' };
        line.split(delimiter)
            .map(|col| col.trim().to_string())
            .collect()
    }

    fn parse_data_line(line: &str, headers: &[String]) -> Option<HashMap<String, String>> {
        let mut line = line;
        // Remove trailing delimiter
        if line.ends_with('|') || line.ends_with('\t') {
            line = &line[..line.len() - 1];
        }

        let delimiter = if line.contains('\t') { '\t' } else { '|' };
        let values: Vec<&str> = line.split(delimiter).collect();

        if values.len() != headers.len() {
            return None;
        }

        let mut map = HashMap::new();
        for (header, value) in headers.iter().zip(values.iter()) {
            map.insert(header.clone(), value.to_string());
        }
        Some(map)
    }

    fn group_jobs_and_steps(raw_jobs: Vec<HashMap<String, String>>) -> Vec<SlurmJob> {
        let mut job_groups: HashMap<String, JobGroup> = HashMap::new();

        for job_data in &raw_jobs {
            let job_id_str = job_data.get("JobID").map(|s| s.as_str()).unwrap_or("");
            if job_id_str.is_empty() {
                continue;
            }

            let is_step = job_id_str.contains('.');
            let is_array_task = job_id_str.contains('_') && !is_step;

            if is_step {
                // Steps belong to their parent job
                let base_id = job_id_str.split('.').next().unwrap_or("").to_string();
                if !base_id.is_empty() {
                    let group = job_groups.entry(base_id).or_default();
                    group.steps.push(job_data.clone());
                }
            } else {
                // Both regular jobs and array tasks are main jobs
                let job_id = if is_array_task {
                    job_id_str.to_string()
                } else {
                    job_id_str.split('.').next().unwrap_or("").to_string()
                };
                let group = job_groups.entry(job_id).or_default();
                group.main = Some(job_data.clone());
            }
        }

        let mut jobs = Vec::new();
        for (_job_id, group) in job_groups {
            if let Some(main_job) = &group.main {
                let mut slurm_job = Self::convert_to_slurm_job(main_job);

                // Merge step data
                for step_data in &group.steps {
                    let mut step_data_owned = step_data.clone();
                    // Inherit user from main job if missing
                    if step_data_owned
                        .get("User")
                        .map_or(true, |u| u.trim().is_empty())
                    {
                        if let Some(user) = main_job.get("User") {
                            step_data_owned.insert("User".to_string(), user.clone());
                        }
                    }

                    let step_job = Self::convert_to_slurm_job(&step_data_owned);
                    slurm_job.steps.extend(step_job.steps);
                }

                jobs.push(slurm_job);
            }
        }

        jobs
    }

    fn parse_job_id(job_id_str: &str) -> JobId {
        if job_id_str.is_empty() {
            return JobId::Numeric(0);
        }

        // Remove step suffix
        let base_id = job_id_str.split('.').next().unwrap_or("");

        // Handle job arrays
        if base_id.contains('_') {
            if base_id.contains('[') {
                // Array range like '1234_[5-10]' - extract base ID
                let numeric_part = base_id.split('_').next().unwrap_or("0");
                return match numeric_part.parse::<i64>() {
                    Ok(n) => JobId::Numeric(n),
                    Err(_) => JobId::Numeric(0),
                };
            } else {
                // Individual array task like '1234_56'
                return JobId::ArrayTask(base_id.to_string());
            }
        }

        match base_id.parse::<i64>() {
            Ok(n) => JobId::Numeric(n),
            Err(_) => JobId::Numeric(0),
        }
    }

    fn convert_to_slurm_job(job_data: &HashMap<String, String>) -> SlurmJob {
        let job_id = Self::parse_job_id(job_data.get("JobID").map(|s| s.as_str()).unwrap_or("0"));
        let user = job_data.get("User").cloned().unwrap_or_default();
        let account = job_data.get("Account").cloned().unwrap_or_default();
        let job_name = job_data.get("JobName").cloned().unwrap_or_default();
        let partition = job_data.get("Partition").cloned().unwrap_or_default();

        // Parse job state
        let state_str = job_data
            .get("State")
            .cloned()
            .unwrap_or_else(|| "UNKNOWN".to_string());
        let state = State {
            current: vec![state_str.clone()],
            reason: String::new(),
        };

        // Parse time information
        let elapsed_seconds =
            Self::parse_time_to_seconds(job_data.get("Elapsed").map(|s| s.as_str()).unwrap_or("0"));

        // Parse time limit
        let time_limit_str = job_data.get("Timelimit").map(|s| s.as_str()).unwrap_or("");
        let time_limit = if !time_limit_str.is_empty() && time_limit_str != "UNLIMITED" {
            let limit_seconds = Self::parse_time_to_seconds(time_limit_str);
            if limit_seconds > 0 {
                Some(NumberValue {
                    set: true,
                    infinite: false,
                    number: limit_seconds / 60, // Convert to minutes
                })
            } else {
                None
            }
        } else {
            None
        };

        let time_info = TimeInfo {
            elapsed: elapsed_seconds,
            start: 0,
            end: 0,
            limit: time_limit,
            ..Default::default()
        };

        // Parse TRES information
        let alloc_tres_str = job_data.get("AllocTRES").map(|s| s.as_str()).unwrap_or("");
        let allocated_tres = TresParser::parse_tres_string(alloc_tres_str);

        let tres_info = TresInfo {
            allocated: if allocated_tres.is_empty() {
                None
            } else {
                Some(allocated_tres)
            },
            ..Default::default()
        };

        // Parse usage strings for job step
        let usage_in_max_str = job_data
            .get("TresUsageInMax")
            .or_else(|| job_data.get("TRESUsageInMax"))
            .map(|s| s.as_str())
            .unwrap_or("");
        let usage_out_max_str = job_data
            .get("TresUsageOutMax")
            .or_else(|| job_data.get("TRESUsageOutMax"))
            .map(|s| s.as_str())
            .unwrap_or("");

        let step_tres = if !usage_in_max_str.is_empty() || !usage_out_max_str.is_empty() {
            let usage_str = if !usage_in_max_str.is_empty() {
                usage_in_max_str
            } else {
                usage_out_max_str
            };
            let consumed_tres = Self::parse_usage_string(usage_str);
            Some(TresInfo {
                consumed: Some(TresData::Resources(consumed_tres)),
                ..Default::default()
            })
        } else {
            None
        };

        // Parse node information
        let node_list_str = job_data.get("NodeList").map(|s| s.as_str()).unwrap_or("");
        let step_nodes = if !node_list_str.is_empty() && !node_list_str.trim().is_empty() {
            let mut map = HashMap::new();
            map.insert(
                "range".to_string(),
                serde_json::Value::String(node_list_str.trim().to_string()),
            );
            Some(map)
        } else {
            None
        };

        let job_step = JobStep {
            time: Some(time_info.clone()),
            tres: step_tres,
            state: Some(vec![state_str.clone()]),
            nodes: step_nodes,
            ..Default::default()
        };

        // Create association
        let association = Association {
            account: account.clone(),
            partition: partition.clone(),
            user: user.clone(),
            ..Default::default()
        };

        SlurmJob {
            account,
            job_id,
            name: job_name,
            partition,
            state,
            time: time_info,
            exit_code: ExitCode::default(),
            tres: tres_info,
            steps: vec![job_step],
            association,
            allocation_nodes: None,
        }
    }

    fn parse_usage_string(usage_str: &str) -> Vec<TresResource> {
        if usage_str.is_empty() {
            return Vec::new();
        }

        let mut resources = Vec::new();
        let registry_ref = TresRegistry::get_instance();

        for cap in TRES_PATTERN.captures_iter(usage_str) {
            let key = cap[1].trim();
            let value = cap[2].trim();

            match key {
                "cpu" => {
                    let cpu_seconds = Self::parse_time_to_seconds(value);
                    resources.push(TresResource {
                        res_type: "cpu".to_string(),
                        name: "cpu".to_string(),
                        id: registry_ref.get_tres_id("cpu", "").unwrap_or(1),
                        count: cpu_seconds as f64,
                        task: None,
                        node: None,
                    });
                }
                "mem" => {
                    let mem_mb = Self::parse_memory_to_mb(value);
                    resources.push(TresResource {
                        res_type: "mem".to_string(),
                        name: "mem".to_string(),
                        id: registry_ref.get_tres_id("mem", "").unwrap_or(2),
                        count: mem_mb as f64,
                        task: None,
                        node: None,
                    });
                }
                "gres/gpuutil" => {
                    if let Ok(gpu_util) = value.parse::<f64>() {
                        resources.push(TresResource {
                            res_type: "gres".to_string(),
                            name: "gpuutil".to_string(),
                            id: registry_ref
                                .get_tres_id("gres", "gpuutil")
                                .unwrap_or(1010),
                            count: gpu_util,
                            task: None,
                            node: None,
                        });
                    }
                }
                "gres/gpumem" => {
                    let gpu_mem_mb = Self::parse_memory_to_mb(value);
                    resources.push(TresResource {
                        res_type: "gres".to_string(),
                        name: "gpumem".to_string(),
                        id: registry_ref
                            .get_tres_id("gres", "gpumem")
                            .unwrap_or(1009),
                        count: gpu_mem_mb as f64,
                        task: None,
                        node: None,
                    });
                }
                k if k.starts_with("gres/gpu:") => {
                    if let Ok(gpu_val) = value.parse::<f64>() {
                        resources.push(TresResource {
                            res_type: "gres".to_string(),
                            name: "gpu".to_string(),
                            id: registry_ref
                                .get_tres_id("gres", "gpu")
                                .unwrap_or(1001),
                            count: gpu_val,
                            task: None,
                            node: None,
                        });
                    }
                }
                _ => {}
            }
        }

        resources
    }

    fn parse_time_to_seconds(time_str: &str) -> i64 {
        let time_str = time_str.trim();
        if time_str.is_empty()
            || time_str == "UNKNOWN"
            || time_str == "Partition_Limit"
            || time_str == "UNLIMITED"
        {
            return 0;
        }

        if !time_str.chars().any(|c| c.is_ascii_digit()) {
            return 0;
        }

        // Handle DD-HH:MM:SS format
        let (days, time_part) = if time_str.contains('-') {
            let parts: Vec<&str> = time_str.splitn(2, '-').collect();
            if parts.len() == 2 {
                let days = parts[0].parse::<i64>().unwrap_or(0);
                (days, parts[1])
            } else {
                (0, time_str)
            }
        } else {
            (0, time_str)
        };

        let time_parts: Vec<&str> = time_part.split(':').collect();
        match time_parts.len() {
            3 => {
                let hours = time_parts[0].parse::<i64>().unwrap_or(0);
                let minutes = time_parts[1].parse::<i64>().unwrap_or(0);
                let seconds = time_parts[2].parse::<i64>().unwrap_or(0);
                days * 86400 + hours * 3600 + minutes * 60 + seconds
            }
            2 => {
                let hours = time_parts[0].parse::<i64>().unwrap_or(0);
                let minutes = time_parts[1].parse::<i64>().unwrap_or(0);
                days * 86400 + hours * 3600 + minutes * 60
            }
            1 => time_parts[0].parse::<i64>().unwrap_or(0),
            _ => 0,
        }
    }

    fn parse_memory_to_mb(mem_str: &str) -> i64 {
        let mem_str = mem_str.trim();
        if mem_str.is_empty() {
            return 0;
        }

        if let Some(cap) = MEMORY_UNIT_PATTERN.captures(mem_str) {
            let value: f64 = cap[1].parse().unwrap_or(0.0);
            let unit = cap.get(2).map_or("", |m| m.as_str());

            let mb = match unit.to_uppercase().as_str() {
                "K" => value / 1024.0,
                "M" | "" => value,
                "G" => value * 1024.0,
                "T" => value * 1024.0 * 1024.0,
                _ => value,
            };

            return mb as i64;
        }

        // Try plain number (assume MB)
        mem_str.parse::<f64>().map(|v| v as i64).unwrap_or(0)
    }

    /// Parse squeue output into job dictionaries.
    pub fn parse_squeue_output(output: &str) -> Vec<HashMap<String, String>> {
        let mut jobs = Vec::new();
        let lines: Vec<&str> = output.trim().lines().collect();

        if lines.is_empty() {
            return jobs;
        }

        // Skip header line
        let data_lines = if lines.len() > 1
            && !lines[0]
                .trim()
                .chars()
                .next()
                .map_or(false, |c| c.is_ascii_digit())
        {
            &lines[1..]
        } else {
            &lines[..]
        };

        for line in data_lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.splitn(5, ',').collect();
            if parts.len() >= 5 {
                let mut job_dict = HashMap::new();
                job_dict.insert("job_id".to_string(), parts[0].trim().to_string());
                job_dict.insert("user_name".to_string(), parts[1].trim().to_string());
                job_dict.insert("state".to_string(), parts[2].trim().to_string());
                job_dict.insert("nodes".to_string(), parts[3].trim().to_string());
                job_dict.insert("tres_req_str".to_string(), parts[4].trim().to_string());
                jobs.push(job_dict);
            }
        }

        jobs
    }

    /// Extract GPU count and type from TRES allocation string.
    pub fn extract_gpu_info_from_tres(tres_str: &str) -> (i32, String) {
        let mut gpu_count = 0;
        let mut gpu_type = "unknown".to_string();

        if tres_str.is_empty() || !tres_str.contains("gres/gpu") {
            return (gpu_count, gpu_type);
        }

        // Look for gres/gpu=N pattern
        static GPU_COUNT_RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"gres/gpu=(\d+)").unwrap());
        static GPU_TYPE_RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"gres/gpu:([^=,]+)=(\d+)").unwrap());

        if let Some(cap) = GPU_COUNT_RE.captures(tres_str) {
            gpu_count = cap[1].parse::<i32>().unwrap_or(0);
        }

        if let Some(cap) = GPU_TYPE_RE.captures(tres_str) {
            gpu_type = cap[1].to_string();
            let type_count = cap[2].parse::<i32>().unwrap_or(0);
            if gpu_count == 0 {
                gpu_count = type_count;
            }
        }

        (gpu_count, gpu_type)
    }
}

#[derive(Default)]
struct JobGroup {
    main: Option<HashMap<String, String>>,
    steps: Vec<HashMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_header() {
        let header = "JobID\tUser\tAccount\tJobName\tState\t";
        let headers = SlurmJobParser::parse_header(header);
        assert_eq!(headers.len(), 5);
        assert_eq!(headers[0], "JobID");
        assert_eq!(headers[4], "State");
    }

    #[test]
    fn test_parse_time_to_seconds() {
        assert_eq!(SlurmJobParser::parse_time_to_seconds("01:30:45"), 5445);
        assert_eq!(SlurmJobParser::parse_time_to_seconds("1-00:00:00"), 86400);
        assert_eq!(SlurmJobParser::parse_time_to_seconds("2-12:30:00"), 217800);
        assert_eq!(SlurmJobParser::parse_time_to_seconds("00:05:00"), 300);
        assert_eq!(SlurmJobParser::parse_time_to_seconds("UNKNOWN"), 0);
        assert_eq!(SlurmJobParser::parse_time_to_seconds(""), 0);
    }

    #[test]
    fn test_parse_memory_to_mb() {
        assert_eq!(SlurmJobParser::parse_memory_to_mb("32G"), 32768);
        assert_eq!(SlurmJobParser::parse_memory_to_mb("1024M"), 1024);
        assert_eq!(SlurmJobParser::parse_memory_to_mb("1156K"), 1);
        assert_eq!(SlurmJobParser::parse_memory_to_mb(""), 0);
    }

    #[test]
    fn test_parse_job_id_numeric() {
        match SlurmJobParser::parse_job_id("1234") {
            JobId::Numeric(n) => assert_eq!(n, 1234),
            _ => panic!("Expected Numeric"),
        }
    }

    #[test]
    fn test_parse_job_id_array_task() {
        match SlurmJobParser::parse_job_id("1234_56") {
            JobId::ArrayTask(s) => assert_eq!(s, "1234_56"),
            _ => panic!("Expected ArrayTask"),
        }
    }

    #[test]
    fn test_parse_job_id_with_step() {
        match SlurmJobParser::parse_job_id("1234.batch") {
            JobId::Numeric(n) => assert_eq!(n, 1234),
            _ => panic!("Expected Numeric"),
        }
    }

    #[test]
    fn test_parse_string_basic() {
        let sacct_output = "JobID\tUser\tAccount\tJobName\tState\tElapsed\tStart\tEnd\tPartition\tAllocCPUS\tAllocNodes\tReqMem\tTimelimit\tAllocTRES\tTresUsageInMax\tTresUsageOutMax\tNodeList\t\n\
                            1234\twjs\tusers\ttest_job\tCOMPLETED\t01:00:00\t2025-01-01\t2025-01-01\tgpu-common\t4\t1\t16G\t05:00:00\tcpu=4,mem=16G,gres/gpu:a100=1\tcpu=00:30:00,mem=8G,gres/gpuutil=85,gres/gpumem=10000M\t\tnode01\t\n";

        let jobs = SlurmJobParser::parse_string(sacct_output).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].association.user, "wjs");
    }

    #[test]
    fn test_extract_gpu_info_from_tres() {
        let (count, gpu_type) = SlurmJobParser::extract_gpu_info_from_tres(
            "cpu=4,mem=64G,node=1,billing=4,gres/gpu=1,gres/gpu:a5000=1",
        );
        assert_eq!(count, 1);
        assert_eq!(gpu_type, "a5000");
    }
}
