use std::collections::HashMap;
use std::process::Command;

use crate::models::QueuedJobInfo;
use crate::parser::SlurmJobParser;

/// Parse Slurm array task string and return count of pending tasks.
pub fn parse_array_task_string(task_string: &str) -> i32 {
    if task_string.is_empty() {
        return 1;
    }

    // Remove % concurrent limit suffix (e.g., "21-100%20" -> "21-100")
    let base_string = task_string.split('%').next().unwrap_or("");

    let mut total = 0i32;
    for part in base_string.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if part.contains('-') {
            // Range: "234-2999" or "1-10:2" (with optional step)
            let range_part: Vec<&str> = part.splitn(2, ':').collect();
            let step: i32 = if range_part.len() > 1 {
                range_part[1].parse().unwrap_or(1)
            } else {
                1
            };

            let bounds: Vec<&str> = range_part[0].splitn(2, '-').collect();
            if bounds.len() == 2 {
                if let (Ok(start), Ok(end)) = (bounds[0].parse::<i32>(), bounds[1].parse::<i32>())
                {
                    if step > 0 {
                        total += (end - start) / step + 1;
                    } else {
                        total += 1;
                    }
                } else {
                    total += 1;
                }
            } else {
                total += 1;
            }
        } else {
            total += 1;
        }
    }

    total.max(1) // At least 1 task
}

pub struct QueuedJobsCollector;

impl QueuedJobsCollector {
    /// Get detailed information about queued/pending jobs.
    pub fn get_queued_jobs_info(
        debug: bool,
        partitions: Option<&[String]>,
    ) -> Vec<QueuedJobInfo> {
        let mut queued_jobs = Vec::new();

        let mut cmd = Command::new("/usr/bin/squeue");
        cmd.args(["--json", "-t", "PENDING"]);

        if let Some(parts) = partitions {
            let partition_str = parts.join(",");
            cmd.args(["-p", &partition_str]);
        }

        if debug {
            eprintln!("Debug: Running squeue --json for pending jobs");
        }

        match cmd.output() {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let data: serde_json::Value = match serde_json::from_str(&stdout) {
                    Ok(v) => v,
                    Err(e) => {
                        if debug {
                            eprintln!("Warning: Error parsing squeue JSON: {}", e);
                        }
                        return queued_jobs;
                    }
                };

                if let Some(jobs) = data.get("jobs").and_then(|v| v.as_array()) {
                    for job in jobs {
                        let job_id = job
                            .get("job_id")
                            .and_then(|v| v.as_i64())
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        let user = job
                            .get("user_name")
                            .and_then(|v| v.as_str())
                            .or_else(|| job.get("user").and_then(|v| v.as_str()))
                            .unwrap_or("")
                            .to_string();
                        let partition = job
                            .get("partition")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        // Get job state
                        let state = job
                            .get("job_state")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|v| v.as_str())
                            .unwrap_or("PENDING")
                            .to_string();

                        // Get queue reason
                        let reason = job
                            .get("state_reason")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        // Get per-task CPU request
                        let per_task_cpus = job
                            .get("cpus")
                            .and_then(|v| v.get("number"))
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0) as i32;

                        // Parse GPU info from tres_per_task or tres_req_str
                        let tres_per_task = job
                            .get("tres_per_task")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let tres_req_str = job
                            .get("tres_req_str")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        let mut gpu_count = 0i32;
                        let mut gpu_type = "unknown".to_string();

                        if !tres_per_task.is_empty() {
                            let (count, gtype) =
                                SlurmJobParser::extract_gpu_info_from_tres(tres_per_task);
                            gpu_count = count;
                            gpu_type = gtype;
                        }
                        if gpu_count == 0 && !tres_req_str.is_empty() {
                            let (count, gtype) =
                                SlurmJobParser::extract_gpu_info_from_tres(tres_req_str);
                            gpu_count = count;
                            gpu_type = gtype;
                        }

                        let per_task_gpus = gpu_count;

                        // Get array task information
                        let array_task_string = job
                            .get("array_task_string")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let is_array_job = !array_task_string.is_empty();
                        let array_task_count = if is_array_job {
                            parse_array_task_string(&array_task_string)
                        } else {
                            1
                        };

                        // Calculate total resources (per-task * task count)
                        let total_cpus = per_task_cpus * array_task_count;
                        let total_gpus = per_task_gpus * array_task_count;

                        let queued_job = QueuedJobInfo {
                            job_id,
                            user,
                            partition,
                            state,
                            cpu_request: total_cpus,
                            gpu_request: total_gpus,
                            gpu_type_request: if gpu_type != "unknown" {
                                Some(gpu_type)
                            } else {
                                None
                            },
                            queue_reason: reason,
                            submit_time: None,
                            is_array_job,
                            array_task_count,
                            array_task_string: if is_array_job {
                                Some(array_task_string)
                            } else {
                                None
                            },
                            per_task_cpus,
                            per_task_gpus,
                        };
                        queued_jobs.push(queued_job);

                        if debug && queued_jobs.len() <= 5 {
                            let last = queued_jobs.last().unwrap();
                            if last.is_array_job {
                                eprintln!(
                                    "Debug: Queued array job {} by {}: {} tasks x ({} CPUs, {} GPUs) = {} total CPUs, {} total GPUs",
                                    last.job_id, last.user, last.array_task_count,
                                    last.per_task_cpus, last.per_task_gpus,
                                    last.cpu_request, last.gpu_request
                                );
                            } else {
                                eprintln!(
                                    "Debug: Queued job {} by {}: {} CPUs, {} GPUs",
                                    last.job_id, last.user, last.cpu_request, last.gpu_request
                                );
                            }
                        }
                    }
                }
            }
            Ok(output) => {
                if debug {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("Warning: Failed to get queued jobs: {}", stderr);
                }
            }
            Err(e) => {
                if debug {
                    eprintln!("Warning: Error getting queued jobs: {}", e);
                }
            }
        }

        queued_jobs
    }

    /// Get count of queued tasks by user.
    pub fn get_queued_jobs_by_user(
        debug: bool,
        partitions: Option<&[String]>,
    ) -> HashMap<String, usize> {
        let queued_jobs = Self::get_queued_jobs_info(debug, partitions);
        let mut user_counts: HashMap<String, usize> = HashMap::new();

        for job in &queued_jobs {
            *user_counts.entry(job.user.clone()).or_insert(0) += job.array_task_count as usize;
        }

        user_counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_array_task_string_range() {
        assert_eq!(parse_array_task_string("234-2999"), 2766);
    }

    #[test]
    fn test_parse_array_task_string_with_limit() {
        assert_eq!(parse_array_task_string("21-100%20"), 80);
    }

    #[test]
    fn test_parse_array_task_string_individual() {
        assert_eq!(parse_array_task_string("1,3,5,7"), 4);
    }

    #[test]
    fn test_parse_array_task_string_with_step() {
        assert_eq!(parse_array_task_string("1-10:2"), 5);
    }

    #[test]
    fn test_parse_array_task_string_empty() {
        assert_eq!(parse_array_task_string(""), 1);
    }

    #[test]
    fn test_parse_array_task_string_mixed() {
        // "1-5,10-15,20" -> 5 + 6 + 1 = 12
        assert_eq!(parse_array_task_string("1-5,10-15,20"), 12);
    }
}
