use std::collections::HashMap;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::calculator::gpu_memory_mb;
use crate::command_ext::{run_with_timeout, SLURM_COMMAND_TIMEOUT};
use crate::models::*;
use crate::scontrol_parser::{clean_value, parse_datetime_epoch, parse_duration_minutes, parse_records};
use crate::tres_parser::TresParser;

/// Parsed fields from `scontrol show job` that the sstat path needs.
#[derive(Debug, Clone)]
pub struct JobDetail {
    pub job_id: i64,
    pub user: String,
    pub partition: String,
    pub state: String,
    pub nodes: String,
    pub num_cpus: i32,
    pub tres_alloc: String,
    pub start_epoch: Option<i64>,
    pub time_limit_minutes: Option<i64>,
    pub account: Option<String>,
    pub tres_per_node: String,
}

pub struct SstatMonitor;

impl SstatMonitor {
    /// Get list of job IDs from squeue (running and pending).
    pub fn get_jobs(
        user: Option<&str>,
        partition: Option<&str>,
        max_jobs: usize,
        debug: bool,
    ) -> Vec<String> {
        let mut cmd = Command::new("squeue");
        cmd.args(["--noheader", "--state=RUNNING,PENDING", "-o", "%i"]);

        if let Some(u) = user {
            cmd.args(["-u", u]);
        }
        if let Some(p) = partition {
            cmd.args(["-p", p]);
        }

        if debug {
            eprintln!("Debug: Running squeue for running/pending job IDs");
        }

        match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut job_ids: Vec<String> = stdout
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();

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

    /// Fetch all job details in one `scontrol show job` call, indexed by job ID string.
    ///
    /// This is much more efficient than one call per job and also provides the
    /// `TRES=` field (tres_alloc) that plain `squeue -o` cannot supply.
    pub fn get_all_job_details(debug: bool) -> HashMap<String, JobDetail> {
        let mut cmd = Command::new("scontrol");
        cmd.args(["show", "job"]);

        let output = match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT) {
            Ok(o) if o.status.success() => o,
            Ok(o) => {
                if debug {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    eprintln!("Warning: scontrol show job failed: {}", stderr);
                }
                return HashMap::new();
            }
            Err(e) => {
                eprintln!("Warning: scontrol show job error: {}", e);
                return HashMap::new();
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let records = parse_records(&stdout);

        if debug {
            eprintln!("Debug: scontrol show job returned {} records", records.len());
        }

        let mut map = HashMap::new();
        for rec in records {
            let raw_job_id = match rec.get("JobId").and_then(|s| clean_value(s)) {
                Some(v) => v.to_string(),
                None => continue,
            };
            let job_id_num: i64 = raw_job_id.parse().unwrap_or(0);

            // UserId=name(uid) → extract name before '('
            let user = rec
                .get("UserId")
                .and_then(|s| clean_value(s))
                .map(|s| s.split('(').next().unwrap_or(s).to_string())
                .unwrap_or_default();

            let partition = rec
                .get("Partition")
                .and_then(|s| clean_value(s))
                .unwrap_or("")
                .to_string();

            let state = rec
                .get("JobState")
                .and_then(|s| clean_value(s))
                .unwrap_or("")
                .to_string();

            let nodes = rec
                .get("NodeList")
                .and_then(|s| clean_value(s))
                .unwrap_or("")
                .to_string();

            let num_cpus: i32 = rec
                .get("NumCPUs")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            // TRES= is the full allocated TRES: cpu=16,mem=500G,node=1,...,gres/gpu=4
            let tres_alloc = rec
                .get("TRES")
                .and_then(|s| clean_value(s))
                .unwrap_or("")
                .to_string();

            let start_epoch = rec
                .get("StartTime")
                .and_then(|s| parse_datetime_epoch(s));

            let time_limit_minutes = rec
                .get("TimeLimit")
                .and_then(|s| parse_duration_minutes(s));

            let account = rec
                .get("Account")
                .and_then(|s| clean_value(s))
                .map(|s| s.to_string());

            let tres_per_node = rec
                .get("TresPerNode")
                .and_then(|s| clean_value(s))
                .unwrap_or("")
                .to_string();

            let detail = JobDetail {
                job_id: job_id_num,
                user,
                partition,
                state,
                nodes,
                num_cpus,
                tres_alloc,
                start_epoch,
                time_limit_minutes,
                account,
                tres_per_node,
            };

            map.insert(raw_job_id, detail);
        }

        map
    }

    /// Get job detail for a single job ID (used when a full fetch is not warranted).
    pub fn get_job_detail(job_id: &str, debug: bool) -> Option<JobDetail> {
        let mut cmd = Command::new("scontrol");
        cmd.args(["show", "job", job_id]);

        let output = run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT).ok()?;
        if !output.status.success() {
            if debug {
                eprintln!("Warning: Failed to get job info for {}", job_id);
            }
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let records = parse_records(&stdout);
        let rec = records.into_iter().next()?;

        let raw_job_id = rec.get("JobId").and_then(|s| clean_value(s))?.to_string();
        let job_id_num: i64 = raw_job_id.parse().unwrap_or(0);

        let user = rec
            .get("UserId")
            .and_then(|s| clean_value(s))
            .map(|s| s.split('(').next().unwrap_or(s).to_string())
            .unwrap_or_default();

        Some(JobDetail {
            job_id: job_id_num,
            user,
            partition: rec.get("Partition").and_then(|s| clean_value(s)).unwrap_or("").to_string(),
            state: rec.get("JobState").and_then(|s| clean_value(s)).unwrap_or("").to_string(),
            nodes: rec.get("NodeList").and_then(|s| clean_value(s)).unwrap_or("").to_string(),
            num_cpus: rec.get("NumCPUs").and_then(|s| s.parse().ok()).unwrap_or(0),
            tres_alloc: rec.get("TRES").and_then(|s| clean_value(s)).unwrap_or("").to_string(),
            start_epoch: rec.get("StartTime").and_then(|s| parse_datetime_epoch(s)),
            time_limit_minutes: rec.get("TimeLimit").and_then(|s| parse_duration_minutes(s)),
            account: rec.get("Account").and_then(|s| clean_value(s)).map(|s| s.to_string()),
            tres_per_node: rec.get("TresPerNode").and_then(|s| clean_value(s)).unwrap_or("").to_string(),
        })
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
                    && value
                        .as_bytes()
                        .last()
                        .is_some_and(|b| b.is_ascii_alphabetic())
                {
                    let unit = match value.as_bytes().last() {
                        Some(b) => b.to_ascii_uppercase() as char,
                        None => continue,
                    };
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
            sstat
                .get("TRESUsageInAve")
                .map(|s| s.as_str())
                .unwrap_or(""),
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

        let ave_cpu = sstat.get("AveCPU").map(|s| s.as_str()).unwrap_or("");
        if !ave_cpu.is_empty() && elapsed_seconds > 0 && alloc_cpus > 0 {
            let cpu_seconds = Self::parse_time_to_seconds(ave_cpu);
            if cpu_seconds > 0 {
                let cpu_eff_val =
                    (cpu_seconds as f64 / (elapsed_seconds as f64 * alloc_cpus as f64)) * 100.0;
                cpu_eff = format!("{:.1}%", cpu_eff_val.min(100.0));
            }
        }

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

    /// Create GPUMetrics from a JobDetail and optional sstat data.
    pub fn create_metrics_from_job_and_sstat(
        job: &JobDetail,
        sstat_data: Option<&HashMap<String, String>>,
        node_gpu_map: &HashMap<String, String>,
        debug: bool,
    ) -> Option<GPUMetrics> {
        let job_id_str = job.job_id.to_string();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let (elapsed, elapsed_seconds) = if let Some(start) = job.start_epoch {
            if start > 0 {
                let secs = now - start;
                (
                    format!(
                        "{:02}:{:02}:{:02}",
                        secs / 3600,
                        (secs % 3600) / 60,
                        secs % 60
                    ),
                    secs,
                )
            } else {
                ("00:00:00".to_string(), 0i64)
            }
        } else {
            ("00:00:00".to_string(), 0i64)
        };

        // Detect GPU type and count from TRES allocation
        let resources = TresParser::parse_tres_string(&job.tres_alloc);
        let alloc_gpus = TresParser::extract_gpu_count(&resources);
        let gpu_type = TresParser::extract_gpu_type_from_tres(&resources)
            .filter(|t| t != "gpu")
            .or_else(|| node_gpu_map.get(&job.nodes).cloned())
            .unwrap_or_else(|| "default".to_string());
        let total_gpu_mem_mb = if alloc_gpus > 0 {
            gpu_memory_mb(&gpu_type)
        } else {
            0
        };

        let (gpu_util, gpu_mem, gpu_mem_eff, gpu_eff, cpu_eff, mem_eff) =
            if let Some(sstat) = sstat_data {
                let (gu, gm, gme, ge) = Self::calculate_gpu_metrics(
                    sstat,
                    alloc_gpus,
                    total_gpu_mem_mb,
                    &job_id_str,
                    debug,
                );
                let (ce, me) = Self::calculate_cpu_mem_efficiency(
                    sstat,
                    &job.tres_alloc,
                    elapsed_seconds,
                    job.num_cpus,
                );
                (gu, gm, gme, ge, ce, me)
            } else {
                (
                    "---".into(),
                    "---".into(),
                    "---".into(),
                    "---".into(),
                    "---".into(),
                    "---".into(),
                )
            };

        let time_limit_seconds = job.time_limit_minutes.unwrap_or(0) * 60;
        let time_eff = if time_limit_seconds > 0 && elapsed_seconds > 0 {
            format!(
                "{:.1}%",
                (elapsed_seconds as f64 / time_limit_seconds as f64 * 100.0).min(100.0)
            )
        } else {
            "---".to_string()
        };

        let state = JobState::from(job.state.as_str());

        Some(GPUMetrics {
            job_id: JobId::Numeric(job.job_id),
            user: job.user.clone(),
            state,
            elapsed,
            partition: job.partition.clone(),
            gpu_util,
            gpu_mem,
            gpu_eff,
            gpu_mem_eff,
            time_eff,
            cpu_eff,
            mem_eff,
            node: if job.nodes.is_empty() {
                None
            } else {
                Some(job.nodes.clone())
            },
            gpu_type: if gpu_type != "default" && alloc_gpus > 0 {
                Some(gpu_type)
            } else {
                None
            },
            gpu_count: alloc_gpus,
            account: job.account.clone(),
        })
    }

    /// Parse memory value with optional K/M/G suffix to KB.
    fn parse_memory_with_unit(mem_str: &str) -> Option<f64> {
        if mem_str.is_empty() {
            return None;
        }
        if mem_str
            .as_bytes()
            .last()
            .is_some_and(|b| b.is_ascii_alphabetic())
        {
            let unit = match mem_str.as_bytes().last() {
                Some(b) => b.to_ascii_uppercase() as char,
                None => return None,
            };
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

        eprintln!("Monitoring {} jobs...", jobs_to_monitor.len());

        // One scontrol call for all job details
        let all_details = Self::get_all_job_details(debug);

        let mut failed_sstat = 0;

        for (i, job_id) in jobs_to_monitor.iter().enumerate() {
            let job = match all_details.get(job_id) {
                Some(d) => d,
                None => {
                    // Fall back to individual lookup (e.g. job finished between squeue and scontrol)
                    match Self::get_job_detail(job_id, debug) {
                        Some(d) => {
                            eprint!(
                                "\rProcessing job {} ({}/{})...",
                                job_id,
                                i + 1,
                                jobs_to_monitor.len()
                            );
                            let sstat_data = if d.state == "RUNNING" {
                                Self::run_sstat(job_id, debug)
                            } else {
                                None
                            };
                            if let Some(m) = Self::create_metrics_from_job_and_sstat(
                                &d,
                                sstat_data.as_ref(),
                                node_gpu_map,
                                debug,
                            ) {
                                if sstat_data.is_none() {
                                    failed_sstat += 1;
                                }
                                metrics.push(m);
                            }
                            continue;
                        }
                        None => {
                            if debug {
                                eprintln!("Warning: Could not get info for job {}", job_id);
                            }
                            continue;
                        }
                    }
                }
            };

            eprint!(
                "\rProcessing job {} ({}/{})...",
                job_id,
                i + 1,
                jobs_to_monitor.len()
            );

            let is_running = job.state == "RUNNING";
            let sstat_data = if is_running {
                Self::run_sstat(job_id, debug)
            } else {
                None
            };

            if let Some(m) = Self::create_metrics_from_job_and_sstat(
                job,
                sstat_data.as_ref(),
                node_gpu_map,
                debug,
            ) {
                if sstat_data.is_none() {
                    failed_sstat += 1;
                }
                metrics.push(m);
            } else if debug {
                eprintln!("Warning: Could not create metrics for job {}", job_id);
            }
        }

        eprintln!();

        if failed_sstat > 0 {
            eprintln!(
                "Note: Could not get detailed statistics for {} jobs (likely not owned by current user)",
                failed_sstat
            );
        }

        metrics
    }
}

#[cfg(test)]
mod tests {
    const JOB_RECORD: &str = r"JobId=49486060 JobName=run_midway.sh
   UserId=anthonyz(1544495742) GroupId=anthonyz(1544495742) MCS_label=N/A
   Priority=142870 Nice=0 Account=pi-pedramh QOS=pedramh-gpu
   JobState=RUNNING Reason=None Dependency=(null)
   RunTime=21:59:07 TimeLimit=3-00:00:00 TimeMin=N/A
   StartTime=2026-05-11T13:32:02 EndTime=2026-05-14T13:32:02
   Partition=pedramh-gpu AllocNode:Sid=midway3-mgt2:2108563
   NodeList=midway3-0423
   NumNodes=1 NumCPUs=16 NumTasks=4 CPUs/Task=4
   TRES=cpu=16,mem=500G,node=1,billing=16,gres/gpu=4
   TresPerNode=gpu:4";

    #[test]
    fn test_parse_job_record_fields() {
        use crate::scontrol_parser::parse_records;
        let records = parse_records(JOB_RECORD);
        assert_eq!(records.len(), 1);
        let rec = &records[0];

        assert_eq!(rec.get("JobId").map(|s| s.as_str()), Some("49486060"));
        assert_eq!(rec.get("JobState").map(|s| s.as_str()), Some("RUNNING"));
        assert_eq!(rec.get("Partition").map(|s| s.as_str()), Some("pedramh-gpu"));
        assert_eq!(rec.get("NodeList").map(|s| s.as_str()), Some("midway3-0423"));
        assert_eq!(rec.get("NumCPUs").map(|s| s.as_str()), Some("16"));
        assert_eq!(
            rec.get("TRES").map(|s| s.as_str()),
            Some("cpu=16,mem=500G,node=1,billing=16,gres/gpu=4")
        );
        assert_eq!(rec.get("Account").map(|s| s.as_str()), Some("pi-pedramh"));
        assert_eq!(rec.get("TresPerNode").map(|s| s.as_str()), Some("gpu:4"));
    }

    #[test]
    fn test_user_id_parsing() {
        let raw = "anthonyz(1544495742)";
        let user = raw.split('(').next().unwrap_or(raw);
        assert_eq!(user, "anthonyz");
    }

    #[test]
    fn test_time_limit_parsing() {
        use crate::scontrol_parser::parse_duration_minutes;
        assert_eq!(parse_duration_minutes("3-00:00:00"), Some(3 * 1440));
        assert_eq!(parse_duration_minutes("2:00:00"), Some(120));
        assert_eq!(parse_duration_minutes("N/A"), None);
    }
}
