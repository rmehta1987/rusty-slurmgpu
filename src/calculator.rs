use std::collections::HashMap;

use crate::constants::*;
use crate::models::*;
use crate::slurm_utils::PartitionTimeLimits;
use crate::tres_parser::TresParser;

/// GPU VRAM sizes in MB, used to calculate GPUMemEff when Slurm doesn't report
/// memory allocation directly. Values sourced from NVIDIA product specifications.
///
/// | GPU            | VRAM (GB) | VRAM (MB) |
/// |----------------|-----------|-----------|
/// | K80            | 12        | 12288     |
/// | P100           | 16        | 16384     |
/// | RTX 2080       | 11        | 11264     |
/// | A5000          | 24        | 24576     |
/// | V100           | 32        | 32768     |
/// | RTX 5000 Ada   | 32        | 32768     |
/// | A40            | 48        | 49152     |
/// | A6000          | 48        | 49152     |
/// | RTX 8000       | 48        | 49152     |
/// | A100 SXM       | 80        | 81920     |
/// | H100           | 80        | 81920     |
/// | RTX 6000 Pro   | 96        | 98304     |
/// | H200           | 140       | 143360    |
/// | H200 MIG 1g    | 18        | 18432     |
/// | H200 MIG 3g/4g | 71        | 72704     |
pub(crate) fn gpu_memory_mb(gpu_type: &str) -> i64 {
    match gpu_type {
        "a100" | "nvidia_a100-sxm4-80gb" => 81920, // 80 GB
        "a40" => 49152,                            // 48 GB
        "a5000" => 24576,                          // 24 GB
        "a6000" | "6000" | "6000_ada" => 49152,    // 48 GB
        "rtx_6000_pro" | "rtx_pro_6000" | "6000_pro" => 98304, // 96 GB
        "v100" => 32768,                           // 32 GB
        "p100" => 16384,                           // 16 GB
        "k80" => 12288,                            // 12 GB
        "rtx8000" => 49152,                        // 48 GB
        "rtx_2080" | "2080rtx" | "2080" => 11264,  // 11 GB
        "rtx_5000" | "5000_ada" => 32768,          // 32 GB
        "titan_v" => 12288,                        // 12 GB
        "h100" => 81920,                           // 80 GB
        "h200" => 143360,                          // 140 GB
        "h200_1g.18gb" => 18432,                   // 18 GB (MIG)
        "h200_3g.71gb" | "h200_4g.71gb" => 72704,  // 71 GB (MIG)
        _ => 24576,                                // 24 GB default
    }
}

pub(crate) struct EfficiencyCalculator;

impl EfficiencyCalculator {
    /// Format elapsed time in HH:MM:SS format.
    pub fn format_time(seconds: i64) -> String {
        if seconds <= 0 {
            return "---".to_string();
        }
        let hours = seconds / SECONDS_PER_HOUR;
        let minutes = (seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
        let secs = seconds % SECONDS_PER_MINUTE;
        format!("{:02}:{:02}:{:02}", hours, minutes, secs)
    }

    /// Get allocated resources from TRES data.
    #[allow(clippy::collapsible_match)]
    pub fn get_allocated_resources(job: &SlurmJob) -> AllocatedResources {
        let mut resources = AllocatedResources {
            cpu: 0,
            mem: 0,
            gpu: 0,
        };

        if let Some(ref allocated) = job.tres.allocated {
            for resource in allocated {
                match resource.res_type.as_str() {
                    "cpu" => resources.cpu = resource.count as i32,
                    "mem" => resources.mem = resource.count as i64,
                    "gres" => {
                        if resource.name == "gpu"
                            || resource.name.to_lowercase().contains("gpu")
                            || gpu_memory_mb(&resource.name) != 24576
                        {
                            resources.gpu = resource.count as i32;
                        }
                    }
                    _ => {}
                }
            }
        }

        resources
    }

    /// Check if a job requests GPU resources.
    pub fn job_requests_gpu(job: &SlurmJob) -> bool {
        if let Some(ref allocated) = job.tres.allocated {
            let gpu_count = TresParser::extract_gpu_count(allocated);
            if gpu_count > 0 {
                return true;
            }
        }

        if let Some(ref requested) = job.tres.requested {
            let resources = Self::get_tres_resources(requested);
            let gpu_count = TresParser::extract_gpu_count(&resources);
            if gpu_count > 0 {
                return true;
            }
        }

        false
    }

    fn get_tres_resources(tres_data: &TresData) -> Vec<TresResource> {
        match tres_data {
            TresData::Resources(resources) => resources.clone(),
            TresData::Stats(stats) => stats.max.clone().unwrap_or_default(),
        }
    }

    /// Extract GPU utilization and memory usage from job steps.
    pub fn get_gpu_metrics_from_steps(job: &SlurmJob) -> (f64, f64) {
        let mut max_gpu_util = 0.0f64;
        let mut max_gpu_mem = 0.0f64;

        for step in &job.steps {
            let step_tres = match &step.tres {
                Some(t) => t,
                None => continue,
            };

            // Check consumed resources (parseable format)
            if let Some(ref consumed) = step_tres.consumed {
                let resources = match consumed {
                    TresData::Resources(r) => r.clone(),
                    TresData::Stats(s) => s.max.clone().unwrap_or_default(),
                };

                for resource in &resources {
                    if resource.res_type == "gres" {
                        match resource.name.as_str() {
                            "gpuutil" => {
                                max_gpu_util = max_gpu_util.max(resource.count);
                            }
                            "gpumem" => {
                                max_gpu_mem = max_gpu_mem.max(resource.count);
                            }
                            "gpu" => {
                                let util_value = resource.count;
                                let gpu_util_percent = if util_value > GPU_NANOSECOND_THRESHOLD {
                                    (util_value / GPU_NANOSECOND_TO_PERCENT_DIVISOR)
                                        .min(MAX_EFFICIENCY_PERCENT)
                                } else {
                                    util_value.min(MAX_EFFICIENCY_PERCENT)
                                };
                                max_gpu_util = max_gpu_util.max(gpu_util_percent);
                            }
                            "gpu_mem" => {
                                max_gpu_mem = max_gpu_mem.max(resource.count);
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Check requested.max for GPU metrics (JSON format)
            if let Some(ref requested) = step_tres.requested {
                let max_resources = match requested {
                    TresData::Stats(stats) => stats.max.clone().unwrap_or_default(),
                    TresData::Resources(r) => r.clone(),
                };

                for resource in &max_resources {
                    if resource.res_type == "gres" {
                        match resource.name.as_str() {
                            "gpuutil" => {
                                max_gpu_util = max_gpu_util.max(resource.count);
                            }
                            "gpumem" => {
                                max_gpu_mem = max_gpu_mem.max(resource.count / BYTES_PER_MB);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        (max_gpu_util, max_gpu_mem)
    }

    /// Extract CPU and memory utilization from job steps.
    pub fn get_cpu_and_mem_metrics(job: &SlurmJob) -> (f64, f64) {
        let mut max_cpu_time_seconds = 0.0f64;
        let mut max_mem_usage_bytes = 0.0f64;
        let allocated = Self::get_allocated_resources(job);

        for step in &job.steps {
            let step_tres = match &step.tres {
                Some(t) => t,
                None => continue,
            };

            // Check consumed resources (parseable format)
            if let Some(ref consumed) = step_tres.consumed {
                let resources = match consumed {
                    TresData::Resources(r) => r.clone(),
                    TresData::Stats(s) => s.max.clone().unwrap_or_default(),
                };

                for resource in &resources {
                    match resource.res_type.as_str() {
                        "cpu" => {
                            max_cpu_time_seconds = max_cpu_time_seconds.max(resource.count);
                        }
                        "mem" => {
                            let mem_mb = resource.count;
                            max_mem_usage_bytes = max_mem_usage_bytes.max(mem_mb * BYTES_PER_MB);
                        }
                        _ => {}
                    }
                }
            }
        }

        // Calculate efficiencies
        let cpu_eff = if allocated.cpu > 0 && job.time.elapsed > 0 && max_cpu_time_seconds >= 0.0 {
            ((max_cpu_time_seconds / (allocated.cpu as f64 * job.time.elapsed as f64))
                * MAX_EFFICIENCY_PERCENT)
                .min(MAX_EFFICIENCY_PERCENT)
        } else {
            -1.0
        };

        let mem_eff = if allocated.mem > 0 && max_mem_usage_bytes > 0.0 {
            let max_mem_mb = max_mem_usage_bytes / BYTES_PER_MB;
            ((max_mem_mb / allocated.mem as f64) * MAX_EFFICIENCY_PERCENT)
                .min(MAX_EFFICIENCY_PERCENT)
        } else {
            -1.0
        };

        (cpu_eff, mem_eff)
    }

    /// Calculate time efficiency based on time limit.
    pub fn calculate_time_efficiency(
        job: &SlurmJob,
        partition_limits: &HashMap<String, PartitionTimeLimits>,
    ) -> f64 {
        if job.time.elapsed <= 0 {
            return 0.0;
        }

        // Get time limit from job (in minutes)
        if let Some(ref limit) = job.time.limit {
            if limit.number > 0 {
                let time_limit_seconds = limit.number * SECONDS_PER_MINUTE;
                return ((job.time.elapsed as f64 / time_limit_seconds as f64)
                    * MAX_EFFICIENCY_PERCENT)
                    .min(MAX_EFFICIENCY_PERCENT);
            }
        }

        // Use partition-specific default
        if let Some(partition_info) = partition_limits.get(&job.partition) {
            if let Some(default_minutes) = partition_info.default_minutes {
                if default_minutes > 0 {
                    let time_limit_seconds = default_minutes * SECONDS_PER_MINUTE;
                    return ((job.time.elapsed as f64 / time_limit_seconds as f64)
                        * MAX_EFFICIENCY_PERCENT)
                        .min(MAX_EFFICIENCY_PERCENT);
                }
            }
        }

        // Fallback to default partition time limit
        ((job.time.elapsed as f64 / DEFAULT_PARTITION_TIME_LIMIT_SECONDS as f64)
            * MAX_EFFICIENCY_PERCENT)
            .min(MAX_EFFICIENCY_PERCENT)
    }

    /// Detect GPU type from TRES allocation data.
    pub fn detect_gpu_type(job: &SlurmJob) -> String {
        // Check allocated TRES resources
        if let Some(ref allocated) = job.tres.allocated {
            let gpu_type = Self::extract_gpu_type_from_tres_resources(allocated);
            if gpu_type != "default" {
                return gpu_type;
            }
        }

        // Check requested TRES resources
        if let Some(ref requested) = job.tres.requested {
            let resources = Self::get_tres_resources(requested);
            let gpu_type = Self::extract_gpu_type_from_tres_resources(&resources);
            if gpu_type != "default" {
                return gpu_type;
            }
        }

        "default".to_string()
    }

    fn extract_gpu_type_from_tres_resources(resources: &[TresResource]) -> String {
        for resource in resources {
            if resource.res_type != "gres" || resource.name.is_empty() {
                continue;
            }

            let gpu_type = if resource.name.contains("gpu:") {
                resource
                    .name
                    .split_once(':')
                    .map(|x| x.1)
                    .map(|s| s.to_string())
            } else if resource.name != "gpu" {
                Some(resource.name.clone())
            } else {
                None
            };

            if let Some(ref gt) = gpu_type {
                // Direct lookup against known GPU types
                if gpu_memory_mb(gt) != 24576 || gt == "default" {
                    return gt.clone();
                }
            }
        }

        "default".to_string()
    }

    /// Calculate all efficiency metrics for a job.
    pub fn calculate_metrics(
        job: &SlurmJob,
        partition_limits: &HashMap<String, PartitionTimeLimits>,
        debug: bool,
    ) -> GPUMetrics {
        let allocated = Self::get_allocated_resources(job);
        let state = job
            .state
            .current
            .first()
            .cloned()
            .unwrap_or_else(|| "UNKNOWN".to_string());
        let elapsed_formatted = Self::format_time(job.time.elapsed);
        let gpu_count = allocated.gpu;
        let detected_gpu_type = Self::detect_gpu_type(job);

        // Extract node name
        let node_name = Self::extract_node_name(job);

        // Calculate efficiencies
        let is_pending_or_running = state == "PENDING" || state == "RUNNING";

        let (time_eff, cpu_eff, mem_eff, gpu_eff, gpu_util, gpu_mem_mb, gpu_mem_eff) =
            if is_pending_or_running {
                (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
            } else {
                let time_eff = Self::calculate_time_efficiency(job, partition_limits);
                let (cpu_eff, mem_eff) = Self::get_cpu_and_mem_metrics(job);
                let (gpu_util, gpu_mem_mb) = Self::get_gpu_metrics_from_steps(job);

                let gpu_eff = if gpu_count > 0 && gpu_util > 0.0 {
                    gpu_util / gpu_count as f64
                } else {
                    0.0
                };

                let gpu_mem_eff = if gpu_count > 0 && gpu_mem_mb > 0.0 {
                    let gpu_total_mb = gpu_memory_mb(&detected_gpu_type);
                    let eff = (gpu_mem_mb / (gpu_total_mb as f64 * gpu_count as f64))
                        * MAX_EFFICIENCY_PERCENT;

                    if debug && eff > MAX_EFFICIENCY_PERCENT {
                        eprintln!(
                            "WARNING: Job {} GPU memory efficiency {:.1}% > 100% (GPU type: {}, Memory: {:.0}MB, Total: {}MB, Count: {})",
                            job.job_id, eff, detected_gpu_type, gpu_mem_mb, gpu_total_mb, gpu_count
                        );
                    }

                    eff
                } else {
                    0.0
                };

                (
                    time_eff,
                    cpu_eff,
                    mem_eff,
                    gpu_eff,
                    gpu_util,
                    gpu_mem_mb,
                    gpu_mem_eff,
                )
            };

        // Format strings
        let is_pending = state == "PENDING";

        let format_eff = |value: f64, show_zero: bool| -> String {
            if is_pending_or_running {
                "---".to_string()
            } else if value > 0.0 {
                format!("{:.1}%", value)
            } else if show_zero {
                "0.0%".to_string()
            } else {
                "---".to_string()
            }
        };

        let gpu_mem_str = format_memory_string(gpu_mem_mb);
        let gpu_util_str = if gpu_util > 0.0 {
            format!("{:.0}%", gpu_util)
        } else if is_pending_or_running {
            "---".to_string()
        } else {
            "0.0%".to_string()
        };

        GPUMetrics {
            user: job.association.user.clone(),
            job_id: job.job_id.clone(),
            state: JobState::from(state.as_str()),
            elapsed: if is_pending {
                "---".to_string()
            } else {
                elapsed_formatted
            },
            time_eff: format_eff(time_eff, true),
            cpu_eff: if cpu_eff >= 0.0 {
                format_eff(cpu_eff, true)
            } else {
                "---".to_string()
            },
            mem_eff: if mem_eff >= 0.0 {
                format_eff(mem_eff, true)
            } else {
                "---".to_string()
            },
            gpu_eff: format_eff(gpu_eff, state == "COMPLETED"),
            gpu_mem: gpu_mem_str,
            gpu_util: gpu_util_str,
            gpu_mem_eff: format_eff(gpu_mem_eff, false),
            partition: job.partition.clone(),
            account: Some(job.account.clone()),
            node: node_name,
            gpu_type: if detected_gpu_type != "default" {
                Some(detected_gpu_type)
            } else {
                None
            },
            gpu_count,
        }
    }

    fn extract_node_name(job: &SlurmJob) -> Option<String> {
        for step in &job.steps {
            if let Some(ref node_str) = step.nodes {
                if !node_str.is_empty() {
                    return Some(node_str.clone());
                }
            }
        }
        None
    }

    /// Calculate summary metrics grouped by user and optionally by partition or account.
    pub fn calculate_summary_metrics(
        metrics: &[GPUMetrics],
        by_partition: bool,
        by_account: bool,
        aggregate_by_account_only: bool,
    ) -> Vec<SummaryMetrics> {
        let mut groups: HashMap<String, Vec<&GPUMetrics>> = HashMap::new();

        for metric in metrics {
            let key = if aggregate_by_account_only {
                metric.account.clone().unwrap_or_default()
            } else if by_partition {
                format!("{}|{}", metric.user, metric.partition)
            } else if by_account {
                format!(
                    "{}|{}",
                    metric.user,
                    metric.account.as_deref().unwrap_or("")
                )
            } else {
                metric.user.clone()
            };

            groups.entry(key).or_default().push(metric);
        }

        let mut summaries = Vec::new();

        for (key, group_metrics) in &groups {
            let (user, partition, account) = if aggregate_by_account_only {
                (String::new(), None, Some(key.clone()))
            } else if by_partition {
                let parts: Vec<&str> = key.splitn(2, '|').collect();
                (
                    parts[0].to_string(),
                    Some(parts.get(1).unwrap_or(&"").to_string()),
                    None,
                )
            } else if by_account {
                let parts: Vec<&str> = key.splitn(2, '|').collect();
                (
                    parts[0].to_string(),
                    None,
                    Some(parts.get(1).unwrap_or(&"").to_string()),
                )
            } else {
                (key.clone(), None, None)
            };

            // Count jobs by state
            let mut completed_jobs = 0usize;
            let mut failed_jobs = 0usize;
            let mut running_jobs = 0usize;
            let mut pending_jobs = 0usize;

            for m in group_metrics {
                match &m.state {
                    JobState::Completed => completed_jobs += 1,
                    JobState::Failed => failed_jobs += 1,
                    JobState::Running => running_jobs += 1,
                    JobState::Pending => pending_jobs += 1,
                    _ => {}
                }
            }

            // Calculate total GPU hours
            let mut total_gpu_hours = 0.0f64;
            for m in group_metrics {
                if m.state.is_pending() {
                    continue;
                }
                let elapsed_seconds = Self::parse_elapsed_time(&m.elapsed);
                if elapsed_seconds > 0 {
                    total_gpu_hours += (elapsed_seconds as f64 / 3600.0) * m.gpu_count as f64;
                }
            }

            // Calculate efficiency averages (exclude pending/running)
            let completed_metrics: Vec<&&GPUMetrics> = group_metrics
                .iter()
                .filter(|m| !m.state.is_pending() && !m.state.is_running())
                .collect();

            let avg_gpu_eff = Self::average_percentage(
                &completed_metrics
                    .iter()
                    .map(|m| m.gpu_eff.as_str())
                    .collect::<Vec<_>>(),
            );
            let avg_gpu_mem_eff = Self::average_percentage(
                &completed_metrics
                    .iter()
                    .map(|m| m.gpu_mem_eff.as_str())
                    .collect::<Vec<_>>(),
            );
            let avg_time_eff = Self::average_percentage(
                &completed_metrics
                    .iter()
                    .map(|m| m.time_eff.as_str())
                    .collect::<Vec<_>>(),
            );
            let avg_cpu_eff = Self::average_percentage(
                &completed_metrics
                    .iter()
                    .map(|m| m.cpu_eff.as_str())
                    .collect::<Vec<_>>(),
            );
            let avg_mem_eff = Self::average_percentage(
                &completed_metrics
                    .iter()
                    .map(|m| m.mem_eff.as_str())
                    .collect::<Vec<_>>(),
            );

            summaries.push(SummaryMetrics {
                user,
                partition,
                account,
                job_count: group_metrics.len(),
                total_gpu_hours,
                avg_gpu_eff,
                avg_gpu_mem_eff,
                avg_time_eff,
                avg_cpu_eff,
                avg_mem_eff,
                completed_jobs,
                failed_jobs,
                running_jobs,
                pending_jobs,
            });
        }

        summaries
    }

    /// Parse elapsed time string to seconds.
    pub fn parse_elapsed_time(elapsed_str: &str) -> i64 {
        if elapsed_str == "---" || elapsed_str.is_empty() {
            return 0;
        }
        let parts: Vec<&str> = elapsed_str.split(':').collect();
        if parts.len() == 3 {
            let hours = parts[0].parse::<i64>().unwrap_or(0);
            let minutes = parts[1].parse::<i64>().unwrap_or(0);
            let seconds = parts[2].parse::<i64>().unwrap_or(0);
            hours * SECONDS_PER_HOUR + minutes * SECONDS_PER_MINUTE + seconds
        } else {
            0
        }
    }

    fn average_percentage(percentage_strings: &[&str]) -> f64 {
        if percentage_strings.is_empty() {
            return 0.0;
        }

        let values: Vec<f64> = percentage_strings
            .iter()
            .map(|s| {
                if s.is_empty() || *s == "---" {
                    0.0
                } else {
                    s.trim_end_matches('%').parse::<f64>().unwrap_or(0.0)
                }
            })
            .collect();

        values.iter().sum::<f64>() / values.len() as f64
    }

    /// Calculate time-weighted average across all metrics.
    pub fn calculate_time_weighted_average(metrics: &[GPUMetrics]) -> GPUMetrics {
        if metrics.is_empty() {
            return GPUMetrics {
                user: String::new(),
                job_id: JobId::Numeric(0),
                state: JobState::Other("WEIGHTED AVG".to_string()),
                elapsed: "00:00:00".to_string(),
                time_eff: String::new(),
                cpu_eff: "---".to_string(),
                mem_eff: "---".to_string(),
                gpu_eff: "---".to_string(),
                gpu_util: "---".to_string(),
                gpu_mem_eff: "---".to_string(),
                gpu_mem: "---".to_string(),
                partition: String::new(),
                account: None,
                node: None,
                gpu_type: None,
                gpu_count: 0,
            };
        }

        let mut total_seconds = 0.0f64;
        let mut weighted_sums: HashMap<&str, f64> = HashMap::new();
        let mut metric_counts: HashMap<&str, usize> = HashMap::new();
        let metric_keys = ["cpu_eff", "mem_eff", "gpu_eff", "gpu_util", "gpu_mem_eff"];

        for key in &metric_keys {
            weighted_sums.insert(key, 0.0);
            metric_counts.insert(key, 0);
        }

        for metric in metrics {
            if metric.state.is_running() || metric.state.is_pending() {
                continue;
            }

            let elapsed_seconds = Self::parse_elapsed_time(&metric.elapsed);
            if elapsed_seconds <= 0 {
                continue;
            }

            total_seconds += elapsed_seconds as f64;

            let values = [
                ("cpu_eff", metric.cpu_eff.as_str()),
                ("mem_eff", metric.mem_eff.as_str()),
                ("gpu_eff", metric.gpu_eff.as_str()),
                ("gpu_util", metric.gpu_util.as_str()),
                ("gpu_mem_eff", metric.gpu_mem_eff.as_str()),
            ];

            for (key, value) in &values {
                if let Some(numeric) = Self::parse_percentage(value) {
                    if let Some(sum) = weighted_sums.get_mut(key) {
                        *sum += elapsed_seconds as f64 * numeric;
                    }
                    if let Some(count) = metric_counts.get_mut(key) {
                        *count += 1;
                    }
                }
            }
        }

        let format_avg = |key: &str| -> String {
            if total_seconds > 0.0 && *metric_counts.get(key).unwrap_or(&0) > 0 {
                let avg = weighted_sums.get(key).unwrap_or(&0.0) / total_seconds;
                if avg == 0.0 {
                    "---".to_string()
                } else {
                    format!("{:.1}%", avg)
                }
            } else {
                "---".to_string()
            }
        };

        GPUMetrics {
            user: String::new(),
            job_id: JobId::Numeric(0),
            state: JobState::Other("WEIGHTED AVG".to_string()),
            elapsed: Self::format_time(total_seconds as i64),
            time_eff: String::new(),
            cpu_eff: format_avg("cpu_eff"),
            mem_eff: format_avg("mem_eff"),
            gpu_eff: format_avg("gpu_eff"),
            gpu_util: format_avg("gpu_util"),
            gpu_mem_eff: format_avg("gpu_mem_eff"),
            gpu_mem: "---".to_string(),
            partition: String::new(),
            account: None,
            node: None,
            gpu_type: None,
            gpu_count: 0,
        }
    }

    fn parse_percentage(pct_str: &str) -> Option<f64> {
        if pct_str.is_empty() || pct_str == "---" {
            return None;
        }
        pct_str.trim_end_matches('%').parse::<f64>().ok()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AllocatedResources {
    pub(crate) cpu: i32,
    pub(crate) mem: i64,
    pub(crate) gpu: i32,
}

/// Format memory in MB to human-readable string.
pub(crate) fn format_memory_string(memory_mb: f64) -> String {
    if memory_mb <= 0.0 {
        return "---".to_string();
    }

    if memory_mb >= MB_PER_GB {
        format!("{:.1}G", memory_mb / MB_PER_GB)
    } else {
        format!("{:.0}M", memory_mb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_time() {
        assert_eq!(EfficiencyCalculator::format_time(3661), "01:01:01");
        assert_eq!(EfficiencyCalculator::format_time(0), "---");
        assert_eq!(EfficiencyCalculator::format_time(86400), "24:00:00");
    }

    #[test]
    fn test_gpu_memory_mb() {
        assert_eq!(gpu_memory_mb("a100"), 81920);
        assert_eq!(gpu_memory_mb("v100"), 32768);
        assert_eq!(gpu_memory_mb("unknown_type"), 24576); // default
    }

    #[test]
    fn test_format_memory_string() {
        assert_eq!(format_memory_string(2048.0), "2.0G");
        assert_eq!(format_memory_string(512.0), "512M");
        assert_eq!(format_memory_string(0.0), "---");
    }

    #[test]
    fn test_parse_elapsed_time() {
        assert_eq!(EfficiencyCalculator::parse_elapsed_time("01:30:00"), 5400);
        assert_eq!(EfficiencyCalculator::parse_elapsed_time("---"), 0);
        assert_eq!(EfficiencyCalculator::parse_elapsed_time(""), 0);
    }
}
