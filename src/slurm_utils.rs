use std::collections::HashMap;
use std::process::Command;
use std::sync::OnceLock;

use crate::command_ext::{run_with_timeout, SLURM_COMMAND_TIMEOUT};
use crate::constants::{SECONDS_PER_HOUR, SECONDS_PER_MINUTE};
use crate::errors::GpuReportError;
use crate::models::{GPUMetrics, SummaryMetrics};

/// Cache for partition time limits (thread-safe, initialized once).
static PARTITION_TIME_LIMITS_CACHE: OnceLock<HashMap<String, PartitionTimeLimits>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct PartitionTimeLimits {
    pub default_minutes: Option<i64>,
    pub max_minutes: Option<i64>,
}

/// Get default and max time limits for all partitions.
pub fn get_partition_time_limits(debug: bool) -> HashMap<String, PartitionTimeLimits> {
    PARTITION_TIME_LIMITS_CACHE
        .get_or_init(|| {
            let mut partition_limits = HashMap::new();

            let mut cmd = Command::new("scontrol");
            cmd.args(["show", "partition", "--json"]);
            match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT)
            {
                Ok(output) if output.status.success() => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        if let Some(partitions) =
                            data.get("partitions").and_then(|v| v.as_array())
                        {
                            for partition in partitions {
                                let name = partition
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();

                                let default_minutes = partition
                                    .get("default_time")
                                    .and_then(|v| v.get("number"))
                                    .and_then(|v| v.as_i64());

                                let max_minutes = partition
                                    .get("max_time")
                                    .and_then(|v| v.get("number"))
                                    .and_then(|v| v.as_i64());

                                if debug && default_minutes.is_some() {
                                    eprintln!(
                                        "Debug: Partition {} - Default: {:?} min, Max: {:?} min",
                                        name, default_minutes, max_minutes
                                    );
                                }

                                partition_limits.insert(
                                    name,
                                    PartitionTimeLimits {
                                        default_minutes,
                                        max_minutes,
                                    },
                                );
                            }
                        }
                    }
                }
                Ok(output) => {
                    if debug {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        eprintln!("Warning: scontrol partition failed: {}", stderr);
                    }
                }
                Err(e) => {
                    if debug {
                        eprintln!("Warning: Failed to run scontrol: {}", e);
                    }
                }
            }

            partition_limits
        })
        .clone()
}

/// Build a mapping of node names to GPU types using scontrol.
pub fn build_node_gpu_mapping(debug: bool) -> HashMap<String, String> {
    let mut node_gpu_map = HashMap::new();

    let mut cmd = Command::new("scontrol");
    cmd.args(["show", "node", "--json"]);
    match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT)
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&stdout) {
                if let Some(nodes) = data.get("nodes").and_then(|v| v.as_array()) {
                    for node in nodes {
                        let node_name = node
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let gres = node
                            .get("gres")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        if !gres.is_empty() && gres.contains("gpu:") {
                            let parts: Vec<&str> = gres.split(':').collect();
                            if parts.len() >= 3 {
                                let gpu_type = parts[1];
                                if !gpu_type.is_empty() && gpu_type != "gpu" {
                                    node_gpu_map
                                        .insert(node_name.clone(), gpu_type.to_string());
                                    if debug && node_gpu_map.len() <= 5 {
                                        eprintln!(
                                            "Debug: Node {} has GPU type: {} (from GRES: {})",
                                            node_name, gpu_type, gres
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if debug {
                eprintln!(
                    "Debug: Built mapping for {} nodes with GPUs",
                    node_gpu_map.len()
                );
            }
        }
        Ok(output) => {
            if debug {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!("Warning: Failed to get node info: {}", stderr);
            }
        }
        Err(e) => {
            if debug {
                eprintln!("Warning: Failed to run scontrol: {}", e);
            }
        }
    }

    node_gpu_map
}

/// Run sacct command and return parseable format output.
pub fn run_sacct(
    start_time: Option<&str>,
    end_time: Option<&str>,
    partition: Option<&str>,
    account: Option<&str>,
    user: Option<&str>,
    all_users: bool,
    jobs: Option<&str>,
    user_specified_time: bool,
    debug: bool,
) -> Result<String, GpuReportError> {
    let mut cmd_args = vec![
        "sacct".to_string(),
        "-p".to_string(),
        "--delimiter=\t".to_string(),
        "--format=JobID,User,Account,JobName,State,Elapsed,Start,End,Partition,AllocCPUS,AllocNodes,ReqMem,Timelimit,AllocTRES,TresUsageInMax,TresUsageOutMax,NodeList".to_string(),
    ];

    let mut effective_start = start_time.map(|s| s.to_string());
    let effective_end = end_time.map(|s| s.to_string());

    if let Some(jobs_str) = jobs {
        cmd_args.extend(["-j".to_string(), jobs_str.to_string()]);
    } else {
        // Add all users flag if needed
        if all_users || user.is_none() {
            cmd_args.push("-a".to_string());
        }

        // Handle start time keywords
        if let Some(ref st) = effective_start {
            let resolved = resolve_time_keyword(st);
            cmd_args.extend(["-S".to_string(), resolved.clone()]);
            effective_start = Some(resolved);
        } else if effective_end.is_none() {
            // Default to last 24 hours
            let last_24h = (chrono::Local::now() - chrono::Duration::hours(24))
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string();
            cmd_args.extend(["-S".to_string(), last_24h]);
        }

        // Handle end time
        if let Some(ref et) = effective_end {
            let resolved = resolve_time_keyword(et);
            cmd_args.extend(["-E".to_string(), resolved.clone()]);
            let _ = resolved; // end time already added to cmd
        } else if effective_start.is_some() && user_specified_time {
            // When user specifies start time but not end time
            let end = if effective_start
                .as_ref()
                .map_or(false, |s| s.contains('T') || s.contains(':'))
            {
                chrono::Local::now()
                    .format("%Y-%m-%dT%H:%M:%S")
                    .to_string()
            } else {
                chrono::Local::now().format("%Y-%m-%d").to_string()
            };
            cmd_args.extend(["-E".to_string(), end]);
        }

        // Add truncate flag when user explicitly specified time
        if user_specified_time {
            cmd_args.push("-T".to_string());
        }

        // Add partition filter
        if let Some(p) = partition {
            cmd_args.extend(["--partition".to_string(), p.to_string()]);
        }

        // Add account filter
        if let Some(a) = account {
            cmd_args.extend(["-A".to_string(), a.to_string()]);
        }

        // Add user filter
        if let Some(u) = user {
            cmd_args.extend(["-u".to_string(), u.to_string()]);
        }
    }

    if debug {
        eprintln!("Running command: {}", cmd_args.join(" "));
    }

    let mut cmd = Command::new(&cmd_args[0]);
    cmd.args(&cmd_args[1..]);
    let output = run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT)
        .map_err(|e| {
            GpuReportError::slurm_command("sacct", -1, &e.to_string(), "")
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        eprintln!("Slurm command failed: {}", cmd_args.join(" "));
        eprintln!("Exit code: {}", exit_code);
        if !stderr.is_empty() {
            eprintln!("Error: {}", stderr.trim());
        }

        return Err(GpuReportError::slurm_command(
            &cmd_args.join(" "),
            exit_code,
            stderr.trim(),
            "",
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    if debug {
        eprintln!("Debug: sacct returned {} bytes", stdout.len());
    }

    Ok(stdout)
}

fn resolve_time_keyword(time_str: &str) -> String {
    match time_str.to_lowercase().as_str() {
        "yesterday" => (chrono::Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string(),
        "today" => chrono::Local::now().format("%Y-%m-%d").to_string(),
        "now" => chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string(),
        _ => time_str.to_string(),
    }
}

/// Sort metrics by specified field.
pub fn sort_metrics(metrics: &mut Vec<GPUMetrics>, sort_by: &str, reverse: bool) {
    metrics.sort_by(|a, b| {
        let cmp = match sort_by {
            "user" => a.user.cmp(&b.user),
            "job_id" => {
                let a_key = parse_job_id_sort_key(&a.job_id.to_string());
                let b_key = parse_job_id_sort_key(&b.job_id.to_string());
                a_key.cmp(&b_key)
            }
            "state" => a.state.cmp(&b.state),
            "elapsed" => {
                let a_secs = parse_elapsed_seconds(&a.elapsed);
                let b_secs = parse_elapsed_seconds(&b.elapsed);
                a_secs.cmp(&b_secs)
            }
            "gpu_eff" | "gpu_mem_eff" | "time_eff" | "cpu_eff" | "mem_eff" => {
                let a_val = parse_percentage_for_sort(match sort_by {
                    "gpu_eff" => &a.gpu_eff,
                    "gpu_mem_eff" => &a.gpu_mem_eff,
                    "time_eff" => &a.time_eff,
                    "cpu_eff" => &a.cpu_eff,
                    "mem_eff" => &a.mem_eff,
                    _ => "0",
                });
                let b_val = parse_percentage_for_sort(match sort_by {
                    "gpu_eff" => &b.gpu_eff,
                    "gpu_mem_eff" => &b.gpu_mem_eff,
                    "time_eff" => &b.time_eff,
                    "cpu_eff" => &b.cpu_eff,
                    "mem_eff" => &b.mem_eff,
                    _ => "0",
                });
                a_val
                    .partial_cmp(&b_val)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
            _ => std::cmp::Ordering::Equal,
        };

        if reverse {
            cmp.reverse()
        } else {
            cmp
        }
    });
}

/// Sort summary metrics by specified field.
pub fn sort_summary_metrics(summaries: &mut Vec<SummaryMetrics>, sort_by: &str, reverse: bool) {
    summaries.sort_by(|a, b| {
        let cmp = match sort_by {
            "user" => a.user.cmp(&b.user),
            "job_id" => a.job_count.cmp(&b.job_count),
            "gpu_eff" => a
                .avg_gpu_eff
                .partial_cmp(&b.avg_gpu_eff)
                .unwrap_or(std::cmp::Ordering::Equal),
            "gpu_mem_eff" => a
                .avg_gpu_mem_eff
                .partial_cmp(&b.avg_gpu_mem_eff)
                .unwrap_or(std::cmp::Ordering::Equal),
            "time_eff" => a
                .avg_time_eff
                .partial_cmp(&b.avg_time_eff)
                .unwrap_or(std::cmp::Ordering::Equal),
            "cpu_eff" => a
                .avg_cpu_eff
                .partial_cmp(&b.avg_cpu_eff)
                .unwrap_or(std::cmp::Ordering::Equal),
            "mem_eff" => a
                .avg_mem_eff
                .partial_cmp(&b.avg_mem_eff)
                .unwrap_or(std::cmp::Ordering::Equal),
            "elapsed" => a
                .total_gpu_hours
                .partial_cmp(&b.total_gpu_hours)
                .unwrap_or(std::cmp::Ordering::Equal),
            _ => std::cmp::Ordering::Equal,
        };

        if reverse {
            cmp.reverse()
        } else {
            cmp
        }
    });
}

fn parse_job_id_sort_key(job_id_str: &str) -> (i64, i64) {
    if job_id_str.contains('_') {
        let parts: Vec<&str> = job_id_str.splitn(2, '_').collect();
        let base = parts[0].parse::<i64>().unwrap_or(0);
        let task = if parts.len() > 1 {
            parts[1].parse::<i64>().unwrap_or(0)
        } else {
            0
        };
        (base, task)
    } else {
        (job_id_str.parse::<i64>().unwrap_or(0), 0)
    }
}

fn parse_elapsed_seconds(elapsed: &str) -> i64 {
    if elapsed == "---" || elapsed.is_empty() {
        return -1;
    }
    let parts: Vec<&str> = elapsed.split(':').collect();
    if parts.len() == 3 {
        let hours = parts[0].parse::<i64>().unwrap_or(0);
        let minutes = parts[1].parse::<i64>().unwrap_or(0);
        let seconds = parts[2].parse::<i64>().unwrap_or(0);
        hours * SECONDS_PER_HOUR + minutes * SECONDS_PER_MINUTE + seconds
    } else {
        -1
    }
}

fn parse_percentage_for_sort(value: &str) -> f64 {
    if value == "---" || value.is_empty() {
        return -1.0;
    }
    value.trim_end_matches('%').parse::<f64>().unwrap_or(-1.0)
}
