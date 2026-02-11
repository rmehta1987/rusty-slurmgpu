use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use crate::cli_helpers::{calculate_metrics_for_jobs, ReportOptions};
use crate::gpu_usage::GPUUsageCalculator;
use crate::models::*;
use crate::parser::SlurmJobParser;
use crate::resource_summary::ResourceSummarizer;
use crate::slurm_utils::{build_node_gpu_mapping, run_sacct, sort_metrics};
use crate::sstat::SstatMonitor;

pub struct ReportData {
    pub metrics: Vec<GPUMetrics>,
    pub error: Option<String>,
}

pub struct UsageData {
    pub gpu_summaries: Vec<GPUTypeSummary>,
    pub cpu_summary: CPUTypeSummary,
    pub queue_summary: QueueSummary,
    pub error: Option<String>,
}

pub struct StatData {
    pub metrics: Vec<GPUMetrics>,
    pub error: Option<String>,
}

pub enum DataMessage {
    ReportReady(ReportData),
    UsageReady(Box<UsageData>),
    StatReady(StatData),
}

pub fn fetch_report_async(
    tx: mpsc::Sender<DataMessage>,
    partition: Option<String>,
    user: Option<String>,
    starttime: Option<String>,
) {
    thread::spawn(move || {
        let options = ReportOptions {
            starttime,
            endtime: None,
            partition_filter: partition,
            account_filter: None,
            user,
            allusers: true,
            jobs: None,
            gpu: false,
            gpu_idle: false,
            summary: false,
            summary_by_partition: false,
            summary_by_account: false,
            detailed: false,
            plain: true,
            debug: false,
            output: None,
            filter_state: "all".to_string(),
            min_gpu_eff: None,
            max_jobs: None,
            sort_by: "job_id".to_string(),
            reverse: false,
            show_partition: true,
            telegraf: false,
            user_specified_time: false,
        };

        let result = try_fetch_report(&options);
        let data = match result {
            Ok(metrics) => ReportData {
                metrics,
                error: None,
            },
            Err(e) => ReportData {
                metrics: Vec::new(),
                error: Some(e),
            },
        };
        let _ = tx.send(DataMessage::ReportReady(data));
    });
}

fn try_fetch_report(options: &ReportOptions) -> Result<Vec<GPUMetrics>, String> {
    let sacct_output = run_sacct(
        options.starttime.as_deref(),
        options.endtime.as_deref(),
        options.partition_filter.as_deref(),
        options.account_filter.as_deref(),
        options.user.as_deref(),
        options.allusers,
        options.jobs.as_deref(),
        options.user_specified_time,
        false,
    )
    .map_err(|e| format!("{}", e))?;

    let all_jobs =
        SlurmJobParser::parse_string(&sacct_output).map_err(|e| format!("Parse error: {}", e))?;

    if all_jobs.is_empty() {
        return Ok(Vec::new());
    }

    let mut metrics = calculate_metrics_for_jobs(&all_jobs, false);
    sort_metrics(&mut metrics, &options.sort_by, options.reverse);
    Ok(metrics)
}

pub fn fetch_usage_async(tx: mpsc::Sender<DataMessage>, partitions: Option<Vec<String>>) {
    thread::spawn(move || {
        let result = try_fetch_usage(partitions.as_deref());
        let data = match result {
            Ok((gpu_summaries, cpu_summary, queue_summary)) => UsageData {
                gpu_summaries,
                cpu_summary,
                queue_summary,
                error: None,
            },
            Err(e) => UsageData {
                gpu_summaries: Vec::new(),
                cpu_summary: CPUTypeSummary {
                    total_cpus: 0,
                    used_cpus: 0,
                    available_cpus: 0,
                    avg_cpu_load: 0.0,
                    nodes_with_cpus: Vec::new(),
                },
                queue_summary: QueueSummary {
                    total_jobs: 0,
                    total_cpus: 0,
                    total_gpus: 0,
                    jobs_by_user: HashMap::new(),
                    jobs_by_partition: HashMap::new(),
                    jobs_by_reason: HashMap::new(),
                },
                error: Some(e),
            },
        };
        let _ = tx.send(DataMessage::UsageReady(Box::new(data)));
    });
}

fn try_fetch_usage(
    partitions: Option<&[String]>,
) -> Result<(Vec<GPUTypeSummary>, CPUTypeSummary, QueueSummary), String> {
    let gpu_slots = GPUUsageCalculator::collect_gpu_slots_info(false, partitions);
    let cpu_slots = GPUUsageCalculator::collect_cpu_slots_info(false, partitions);
    let queued_jobs = GPUUsageCalculator::get_queued_jobs_info(false, partitions);

    let gpu_summaries = if gpu_slots.is_empty() {
        Vec::new()
    } else {
        GPUUsageCalculator::summarize_by_type(&gpu_slots)
    };
    let cpu_summary = GPUUsageCalculator::summarize_cpu_usage(&cpu_slots);
    let queue_summary = ResourceSummarizer::summarize_queue_info(&queued_jobs);

    Ok((gpu_summaries, cpu_summary, queue_summary))
}

pub fn fetch_stat_async(
    tx: mpsc::Sender<DataMessage>,
    user: Option<String>,
    partition: Option<String>,
) {
    thread::spawn(move || {
        let result = try_fetch_stat(user.as_deref(), partition.as_deref());
        let data = match result {
            Ok(metrics) => StatData {
                metrics,
                error: None,
            },
            Err(e) => StatData {
                metrics: Vec::new(),
                error: Some(e),
            },
        };
        let _ = tx.send(DataMessage::StatReady(data));
    });
}

fn try_fetch_stat(user: Option<&str>, partition: Option<&str>) -> Result<Vec<GPUMetrics>, String> {
    let node_gpu_map = build_node_gpu_mapping(false);
    let metrics = SstatMonitor::monitor_jobs(user, partition, None, 100, &node_gpu_map, false);
    Ok(metrics)
}
