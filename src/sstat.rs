use std::collections::HashMap;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::calculator::gpu_memory_mb;
use crate::command_ext::{run_with_timeout, SLURM_COMMAND_TIMEOUT};
use crate::models::*;
use crate::tres_parser::TresParser;

pub struct SstatMonitor;

impl SstatMonitor {
    /// Get list of job IDs using squeue (running and pending).
    pub fn get_jobs(
        user: Option<&str>,
        partition: Option<&str>,
        max_jobs: usize,
        debug: bool,
    ) -> Vec<String> {
        let mut cmd = Command::new("squeue");
        cmd.args(["--json", "--state=RUNNING,PENDING"]);

        if let Some(u) = user {
            cmd.args(["-u", u]);
        }
        if let Some(p) = partition {
            cmd.args(["-p", p]);
        }

        if debug {
            eprintln!("Debug: Running squeue --json for running/pending jobs");
        }

        match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let data: serde_json::Value = match serde_json::from_str(&stdout) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Error parsing squeue output: {}", e);
                        return Vec::new();
                    }
                };

                let mut job_ids = Vec::new();
                if let Some(jobs) = data.get("jobs").and_then(|v| v.as_array()) {
                    for job in jobs {
                        if let Some(job_id) = job.get("job_id").and_then(|v| v.as_i64()) {
                            job_ids.push(job_id.to_string());
                        }
                    }
                }

                if debug {
                    eprintln!("Debug: Found {} jobs", job_ids.len());
                }

                if max_jobs > 0 {
                    job_ids.truncate(max_jobs);
                }
                job_ids
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!("Error running squeue: {}", stderr);
                Vec::new()
            }
            Err(e) => {
                eprintln!("Error running squeue: {}", e);
                Vec::new()
            }
        }
    }

    /// Get job information from squeue for a specific job.
    pub fn get_job_info(job_id: &str, debug: bool) -> Option<serde_json::Value> {
        let mut cmd = Command::new("squeue");
        cmd.args(["--json", "-j", job_id]);
        let output = match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT)
        {
            Ok(output) if output.status.success() => output,
            Ok(_) | Err(_) => {
                if debug {
                    eprintln!("Warning: Failed to get job info for {}", job_id);
                }
                return None;
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let data: serde_json::Value = match serde_json::from_str(&stdout) {
            Ok(v) => v,
            Err(_) => return None,
        };

        data.get("jobs")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first().cloned())
    }

    /// Parse TRES usage string into dictionary.
    fn parse_tres_string(tres_str: &str) -> HashMap<String, f64> {
        let mut tres_dict = HashMap::new();
        if tres_str.is_empty() {
            return tres_dict;
        }

        for part in tres_str.split(',') {
            if let Some((key, value)) = part.split_once('=') {
                let parsed = if (key == "mem" || key == "gres/gpumem")
                    && !value.is_empty()
                    && value.as_bytes().last().map_or(false, |b| b.is_ascii_alphabetic())
                {
                    let unit = value.as_bytes().last().unwrap().to_ascii_uppercase() as char;
                    let number: f64 = value[..value.len() - 1].parse().unwrap_or(0.0);
                    let multiplier = match unit {
                        'K' => 1024.0,
                        'M' => 1024.0 * 1024.0,
                        'G' => 1024.0 * 1024.0 * 1024.0,
                        'T' => 1024.0 * 1024.0 * 1024.0 * 1024.0,
                        _ => 1.0,
                    };
                    Some(number * multiplier)
                } else {
                    value.parse::<f64>().ok()
                };

                if let Some(val) = parsed {
                    tres_dict.insert(key.to_string(), val);
                }
            }
        }

        tres_dict
    }

    /// Parse time string (HH:MM:SS or DD-HH:MM:SS) to seconds.
    fn parse_time_to_seconds(time_str: &str) -> i64 {
        if time_str.is_empty() || time_str == "00:00:00" {
            return 0;
        }

        let (days, time_part) = if time_str.contains('-') {
            let parts: Vec<&str> = time_str.splitn(2, '-').collect();
            let days = parts[0].parse::<i64>().unwrap_or(0);
            (days, parts.get(1).copied().unwrap_or(""))
        } else {
            (0, time_str)
        };

        let parts: Vec<&str> = time_part.split(':').collect();
        if parts.len() == 3 {
            let hours = parts[0].parse::<i64>().unwrap_or(0);
            let minutes = parts[1].parse::<i64>().unwrap_or(0);
            let seconds = parts[2].parse::<i64>().unwrap_or(0);
            days * 86400 + hours * 3600 + minutes * 60 + seconds
        } else {
            0
        }
    }

    /// Run sstat command for a specific job and parse output.
    pub fn run_sstat(job_id: &str, debug: bool) -> Option<HashMap<String, String>> {
        let mut cmd = Command::new("sstat");
        cmd.args([
            "-j",
            job_id,
            "-p",
            "--allsteps",
            "-o",
            "JobID,AveCPU,MaxRSS,AveRSS,TRESUsageInMax,TRESUsageInAve,TRESUsageInTot",
        ]);
        let output = run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT).ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.is_empty() {
            if debug && !output.stderr.is_empty() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!("Warning: sstat stderr: {}", stderr);
            }
            return None;
        }

        let lines: Vec<&str> = stdout.trim().lines().collect();
        if lines.len() < 2 {
            return None;
        }

        let headers: Vec<&str> = lines[0].trim_matches('|').split('|').collect();

        // Find the best step (prefer batch over extern)
        let mut best_step: Option<HashMap<String, String>> = None;
        for line in &lines[1..] {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let values: Vec<&str> = line.trim_matches('|').split('|').collect();
            if values.len() >= headers.len() {
                let step_data: HashMap<String, String> = headers
                    .iter()
                    .zip(values.iter())
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();

                let job_step = step_data.get("JobID").map(|s| s.as_str()).unwrap_or("");
                if job_step.contains(".batch") {
                    best_step = Some(step_data);
                    break;
                } else if !job_step.contains(".extern") && best_step.is_none() {
                    best_step = Some(step_data);
                }
            }
        }

        if debug && best_step.is_some() {
            eprintln!("Debug: Got sstat data for job {}", job_id);
        }

        best_step
    }

    /// Calculate GPU utilization and memory metrics from sstat TRES data.
    fn calculate_gpu_metrics(
        sstat: &HashMap<String, String>,
        alloc_gpus: i32,
        total_gpu_mem_mb: i64,
        job_id_str: &str,
        debug: bool,
    ) -> (String, String, String, String) {
        let mut gpu_util = "---".to_string();
        let mut gpu_mem = "---".to_string();
        let mut gpu_mem_eff = "---".to_string();
        let mut gpu_eff = "---".to_string();

        if alloc_gpus == 0 {
            return (gpu_util, gpu_mem, gpu_mem_eff, gpu_eff);
        }

        let tres_ave = Self::parse_tres_string(
            sstat.get("TRESUsageInAve").map(|s| s.as_str()).unwrap_or(""),
        );

        let gpu_util_val = tres_ave.get("gres/gpuutil").copied().unwrap_or(0.0);
        if gpu_util_val > 0.0 {
            gpu_util = format!("{:.1}%", gpu_util_val);
            gpu_eff = gpu_util.clone();
        }

        let gpu_mem_bytes = tres_ave.get("gres/gpumem").copied().unwrap_or(0.0);
        if gpu_mem_bytes > 0.0 {
            let gpu_mem_mb = gpu_mem_bytes / (1024.0 * 1024.0);

            gpu_mem = if gpu_mem_mb >= 1024.0 {
                format!("{:.1}G", gpu_mem_mb / 1024.0)
            } else {
                format!("{}M", gpu_mem_mb as i64)
            };

            if total_gpu_mem_mb > 0 {
                let mem_eff_val =
                    (gpu_mem_mb / (total_gpu_mem_mb as f64 * alloc_gpus as f64)) * 100.0;
                gpu_mem_eff = format!("{:.1}%", mem_eff_val.min(100.0));

                if debug && mem_eff_val > 100.0 {
                    eprintln!(
                        "Debug: Job {} GPU memory efficiency calculated as {:.1}% (capped to 100%)",
                        job_id_str, mem_eff_val
                    );
                }
            }
        }

        (gpu_util, gpu_mem, gpu_mem_eff, gpu_eff)
    }

    /// Calculate CPU and memory efficiency from sstat data.
    fn calculate_cpu_mem_efficiency(
        sstat: &HashMap<String, String>,
        tres_alloc_str: &str,
        elapsed_seconds: i64,
        alloc_cpus: i32,
    ) -> (String, String) {
        let mut cpu_eff = "---".to_string();
        let mut mem_eff = "---".to_string();

        // CPU efficiency
        let ave_cpu = sstat.get("AveCPU").map(|s| s.as_str()).unwrap_or("");
        if !ave_cpu.is_empty() && elapsed_seconds > 0 && alloc_cpus > 0 {
            let cpu_seconds = Self::parse_time_to_seconds(ave_cpu);
            if cpu_seconds > 0 {
                let cpu_eff_val =
                    (cpu_seconds as f64 / (elapsed_seconds as f64 * alloc_cpus as f64)) * 100.0;
                cpu_eff = format!("{:.1}%", cpu_eff_val.min(100.0));
            }
        }

        // Memory efficiency
        let max_rss = sstat.get("MaxRSS").map(|s| s.as_str()).unwrap_or("");
        if !max_rss.is_empty() && max_rss != "0" {
            if let Some(max_rss_kb) = Self::parse_memory_with_unit(max_rss) {
                let tres_dict = Self::parse_tres_string(tres_alloc_str);
                let alloc_mem_bytes = tres_dict.get("mem").copied().unwrap_or(0.0);
                if alloc_mem_bytes > 0.0 && max_rss_kb > 0.0 {
                    let alloc_mem_kb = alloc_mem_bytes / 1024.0;
                    let mem_eff_val = (max_rss_kb / alloc_mem_kb) * 100.0;
                    mem_eff = format!("{:.1}%", mem_eff_val.min(100.0));
                }
            }
        }

        (cpu_eff, mem_eff)
    }

    /// Create GPUMetrics from job info and optional sstat data.
    pub fn create_metrics_from_job_and_sstat(
        job_info: &serde_json::Value,
        sstat_data: Option<&HashMap<String, String>>,
        node_gpu_map: &HashMap<String, String>,
        debug: bool,
    ) -> Option<GPUMetrics> {
        let job_id_num = job_info.get("job_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let job_id_str = job_id_num.to_string();
        let user = job_info
            .get("user_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let partition = job_info
            .get("partition")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let node_name = job_info
            .get("nodes")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let alloc_cpus = job_info
            .get("cpus")
            .and_then(|v| v.get("number"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let tres_alloc_str = job_info
            .get("tres_alloc_str")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Calculate elapsed time
        let start_time = job_info
            .get("start_time")
            .and_then(|v| v.get("number"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let (elapsed, elapsed_seconds) = if start_time > 0 {
            let secs = now - start_time;
            (
                format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60),
                secs,
            )
        } else {
            ("00:00:00".to_string(), 0i64)
        };

        // Detect GPU type and count
        let resources = TresParser::parse_tres_string(tres_alloc_str);
        let alloc_gpus = TresParser::extract_gpu_count(&resources);
        let gpu_type = TresParser::extract_gpu_type_from_tres(&resources)
            .filter(|t| t != "gpu")
            .or_else(|| node_gpu_map.get(&node_name).cloned())
            .unwrap_or_else(|| "default".to_string());
        let total_gpu_mem_mb = if alloc_gpus > 0 { gpu_memory_mb(&gpu_type) } else { 0 };

        // Calculate efficiency metrics from sstat
        let (gpu_util, gpu_mem, gpu_mem_eff, gpu_eff, cpu_eff, mem_eff) =
            if let Some(sstat) = sstat_data {
                let (gu, gm, gme, ge) = Self::calculate_gpu_metrics(
                    sstat, alloc_gpus, total_gpu_mem_mb, &job_id_str, debug,
                );
                let (ce, me) = Self::calculate_cpu_mem_efficiency(
                    sstat, tres_alloc_str, elapsed_seconds, alloc_cpus,
                );
                (gu, gm, gme, ge, ce, me)
            } else {
                ("---".into(), "---".into(), "---".into(), "---".into(), "---".into(), "---".into())
            };

        // Time efficiency
        let time_limit_seconds = job_info
            .get("time_limit")
            .and_then(|v| v.get("number"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            * 60;
        let time_eff = if time_limit_seconds > 0 && elapsed_seconds > 0 {
            format!("{:.1}%", (elapsed_seconds as f64 / time_limit_seconds as f64 * 100.0).min(100.0))
        } else {
            "---".to_string()
        };

        let state = JobState::from(
            job_info
                .get("job_state")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|s| s.as_str())
                .unwrap_or("RUNNING"),
        );

        Some(GPUMetrics {
            job_id: JobId::Numeric(job_id_num),
            user,
            state,
            elapsed,
            partition,
            gpu_util,
            gpu_mem,
            gpu_eff,
            gpu_mem_eff,
            time_eff,
            cpu_eff,
            mem_eff,
            node: if node_name.is_empty() { None } else { Some(node_name) },
            gpu_type: if gpu_type != "default" && alloc_gpus > 0 { Some(gpu_type) } else { None },
            gpu_count: alloc_gpus,
            account: job_info
                .get("account")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
    }

    /// Parse memory value with optional K/M/G suffix to KB.
    fn parse_memory_with_unit(mem_str: &str) -> Option<f64> {
        if mem_str.is_empty() {
            return None;
        }
        if mem_str.as_bytes().last().map_or(false, |b| b.is_ascii_alphabetic()) {
            let unit = mem_str.as_bytes().last().unwrap().to_ascii_uppercase() as char;
            let number: f64 = mem_str[..mem_str.len() - 1].parse().ok()?;
            let multiplier = match unit {
                'K' => 1.0,
                'M' => 1024.0,
                'G' => 1024.0 * 1024.0,
                'T' => 1024.0 * 1024.0 * 1024.0,
                _ => 1.0,
            };
            Some(number * multiplier)
        } else {
            mem_str.parse::<f64>().ok()
        }
    }

    /// Monitor running jobs and return metrics.
    pub fn monitor_jobs(
        user: Option<&str>,
        partition: Option<&str>,
        job_ids: Option<&[String]>,
        max_jobs: usize,
        node_gpu_map: &HashMap<String, String>,
        debug: bool,
    ) -> Vec<GPUMetrics> {
        let mut metrics = Vec::new();

        // Get job IDs to monitor
        let jobs_to_monitor = if let Some(ids) = job_ids {
            let iter = ids.iter();
            if max_jobs > 0 {
                iter.take(max_jobs).cloned().collect::<Vec<_>>()
            } else {
                iter.cloned().collect::<Vec<_>>()
            }
        } else {
            Self::get_jobs(user, partition, max_jobs, debug)
        };

        if jobs_to_monitor.is_empty() {
            return metrics;
        }

        eprintln!(
            "Monitoring {} jobs...",
            jobs_to_monitor.len()
        );

        let mut failed_sstat = 0;

        for (i, job_id) in jobs_to_monitor.iter().enumerate() {
            // Get job info from squeue
            let job_info = match Self::get_job_info(job_id, debug) {
                Some(info) => info,
                None => {
                    if debug {
                        eprintln!("Warning: Could not get info for job {}", job_id);
                    }
                    continue;
                }
            };

            eprint!(
                "\rProcessing job {} ({}/{})...",
                job_id,
                i + 1,
                jobs_to_monitor.len()
            );

            // Only run sstat for running jobs (pending jobs have no stats)
            let is_running = job_info
                .get("job_state")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|s| s.as_str())
                == Some("RUNNING");

            let sstat_data = if is_running {
                Self::run_sstat(job_id, debug)
            } else {
                None
            };

            // Create metrics
            let metric = Self::create_metrics_from_job_and_sstat(
                &job_info,
                sstat_data.as_ref(),
                node_gpu_map,
                debug,
            );

            if let Some(m) = metric {
                if sstat_data.is_none() {
                    failed_sstat += 1;
                }
                metrics.push(m);
            } else if debug {
                eprintln!("Warning: Could not create metrics for job {}", job_id);
            }
        }

        eprintln!(); // Clear the progress line

        if failed_sstat > 0 {
            eprintln!(
                "Note: Could not get detailed statistics for {} jobs (likely not owned by current user)",
                failed_sstat
            );
        }

        metrics
    }
}
