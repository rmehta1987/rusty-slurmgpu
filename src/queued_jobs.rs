use std::collections::HashMap;
use std::process::Command;

use crate::command_ext::{run_with_timeout, SLURM_COMMAND_TIMEOUT};
use crate::gres_parser::GresParser;
use crate::models::QueuedJobInfo;

/// Parse Slurm array task string and return count of pending tasks.
pub(crate) fn parse_array_task_string(task_string: &str) -> i32 {
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
                if let (Ok(start), Ok(end)) = (bounds[0].parse::<i32>(), bounds[1].parse::<i32>()) {
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

/// Return `None` for the Slurm "no value" sentinels used in squeue output.
fn clean_field(s: &str) -> Option<&str> {
    match s {
        "" | "N/A" | "(null)" | "None" => None,
        other => Some(other),
    }
}

pub(crate) struct QueuedJobsCollector;

impl QueuedJobsCollector {
    /// Get detailed information about queued/pending jobs.
    ///
    /// Uses `squeue --noheader -t PENDING` with a fixed pipe-delimited format:
    /// `JobID|User|Partition|State|Reason|CPUs|TRESPerNode|ArrayTaskID`
    pub fn get_queued_jobs_info(debug: bool, partitions: Option<&[String]>) -> Vec<QueuedJobInfo> {
        let mut queued_jobs = Vec::new();

        let mut cmd = Command::new("squeue");
        // %i=JobID, %u=User, %P=Partition, %T=State, %r=Reason,
        // %C=NumCPUs, %b=TRESPerNode (gpu:N), %K=ArrayTaskID
        cmd.args(["--noheader", "-t", "PENDING", "-o", "%i|%u|%P|%T|%r|%C|%b|%K"]);

        if let Some(parts) = partitions {
            let partition_str = parts.join(",");
            cmd.args(["-p", &partition_str]);
        }

        if debug {
            eprintln!("Debug: Running squeue for pending jobs");
        }

        match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let fields: Vec<&str> = line.splitn(8, '|').collect();
                    if fields.len() < 8 {
                        if debug {
                            eprintln!("Warning: unexpected squeue line: {}", line);
                        }
                        continue;
                    }

                    let raw_job_id = fields[0];
                    let user = fields[1].to_string();
                    let partition = fields[2].to_string();
                    let state = fields[3].to_string();
                    let reason = fields[4].to_string();
                    let per_task_cpus: i32 =
                        clean_field(fields[5]).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let tres_per_node = clean_field(fields[6]).unwrap_or("");
                    let array_task_id = clean_field(fields[7]).unwrap_or("");

                    // Strip array spec from job ID if present: "49486702_[106-179%200]" → "49486702"
                    let job_id = raw_job_id
                        .split('_')
                        .next()
                        .unwrap_or(raw_job_id)
                        .to_string();

                    // Parse GPU info from TRESPerNode (%b): "gpu:N" or "gpu:TYPE:N"
                    // %b produces GRES format (gpu:N), not TRES format (gres/gpu=N).
                    let mut gpu_count = 0i32;
                    let mut gpu_type = "unknown".to_string();
                    if !tres_per_node.is_empty() {
                        if let Some((gtype, count)) =
                            GresParser::parse_gres_string(tres_per_node).into_iter().next()
                        {
                            gpu_count = count;
                            if gtype != "gpu" {
                                gpu_type = gtype;
                            }
                        }
                    }
                    let per_task_gpus = gpu_count;

                    // Array task info: %K gives the task range directly
                    let array_task_string = array_task_id.to_string();
                    let is_array_job = !array_task_string.is_empty();
                    let array_task_count = if is_array_job {
                        parse_array_task_string(&array_task_string)
                    } else {
                        1
                    };

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

                    if debug && queued_jobs.len() < 5 {
                        if queued_job.is_array_job {
                            eprintln!(
                                "Debug: Queued array job {} by {}: {} tasks x ({} CPUs, {} GPUs) = {} total CPUs, {} total GPUs",
                                queued_job.job_id, queued_job.user, queued_job.array_task_count,
                                queued_job.per_task_cpus, queued_job.per_task_gpus,
                                queued_job.cpu_request, queued_job.gpu_request
                            );
                        } else {
                            eprintln!(
                                "Debug: Queued job {} by {}: {} CPUs, {} GPUs",
                                queued_job.job_id, queued_job.user,
                                queued_job.cpu_request, queued_job.gpu_request
                            );
                        }
                    }

                    queued_jobs.push(queued_job);
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

    #[test]
    fn test_clean_field_sentinels() {
        assert_eq!(clean_field("N/A"), None);
        assert_eq!(clean_field(""), None);
        assert_eq!(clean_field("(null)"), None);
        assert_eq!(clean_field("gpu:1"), Some("gpu:1"));
    }

    #[test]
    fn test_parse_squeue_pending_line_no_gpu() {
        // Simulate: 49448599|aaz|aaz|PENDING|BeginTime|20|N/A|N/A
        let line = "49448599|aaz|aaz|PENDING|BeginTime|20|N/A|N/A";
        let fields: Vec<&str> = line.splitn(8, '|').collect();
        assert_eq!(fields[0], "49448599");
        assert_eq!(fields[5], "20");
        assert_eq!(clean_field(fields[6]), None); // N/A tres
        assert_eq!(clean_field(fields[7]), None); // N/A array
    }

    #[test]
    fn test_parse_squeue_pending_line_array_job() {
        // Simulate: 49486702_[106-179%200]|zheful|amd|PENDING|...|8|N/A|106-179%200
        let line = "49486702_[106-179%200]|zheful|amd|PENDING|QOSMaxNodePerUserLimit|8|N/A|106-179%200";
        let fields: Vec<&str> = line.splitn(8, '|').collect();
        let raw_job_id = fields[0];
        let job_id = raw_job_id.split('_').next().unwrap_or(raw_job_id);
        assert_eq!(job_id, "49486702");
        let array_task_id = clean_field(fields[7]).unwrap_or("");
        assert_eq!(array_task_id, "106-179%200");
        assert_eq!(parse_array_task_string(array_task_id), 74);
    }

    #[test]
    fn test_parse_squeue_pending_line_gpu() {
        // Simulate: 43900631|lwu12|beagle3|PENDING|...|32|gpu:4|N/A
        // %b produces GRES format "gpu:N", not TRES format "gres/gpu=N"
        let line = "43900631|lwu12|beagle3|PENDING|DependencyNeverSatisfied|32|gpu:4|N/A";
        let fields: Vec<&str> = line.splitn(8, '|').collect();
        let tres = clean_field(fields[6]).unwrap_or("");
        let gpu_info = GresParser::parse_gres_string(tres);
        let count = gpu_info.first().map(|(_, c)| *c).unwrap_or(0);
        assert_eq!(count, 4);
    }
}
