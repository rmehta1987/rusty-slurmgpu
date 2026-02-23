use std::collections::HashMap;

use anyhow::{bail, Context, Result};

use crate::calculator::EfficiencyCalculator;
use crate::constants::{GPU_IDLE_THRESHOLD_PERCENT, GPU_MEMORY_IDLE_THRESHOLD_PERCENT};
use crate::models::{GPUMetrics, SlurmJob};
use crate::parser::SlurmJobParser;
use crate::reporter::GPUReporter;
use crate::slurm_utils::{
    build_node_gpu_mapping, get_partition_time_limits, run_sacct, sort_summary_metrics,
};
use crate::validation::InputValidator;

/// Parsed and validated CLI options for report generation.
#[derive(Debug, Clone)]
pub struct ReportOptions {
    pub starttime: Option<String>,
    pub endtime: Option<String>,
    pub partition_filter: Option<String>,
    pub account_filter: Option<String>,
    pub user: Option<String>,
    pub allusers: bool,
    pub jobs: Option<String>,
    pub gpu: bool,
    pub gpu_idle: bool,
    pub summary: bool,
    pub summary_by_partition: bool,
    pub summary_by_account: bool,
    pub detailed: bool,
    pub plain: bool,
    pub debug: bool,
    pub output: Option<String>,
    pub filter_state: String,
    pub min_gpu_eff: Option<f64>,
    pub max_jobs: Option<usize>,
    pub sort_by: String,
    pub reverse: bool,
    pub show_partition: bool,
    pub telegraf: bool,
    pub user_specified_time: bool,
}

/// Fetch job data from sacct and parse it.
pub fn fetch_and_parse_jobs(options: &ReportOptions) -> Result<Vec<SlurmJob>> {
    // Validate user-provided inputs
    if let Some(ref user) = options.user {
        InputValidator::validate_user_name(user).map_err(|e| anyhow::anyhow!("{}", e))?;
    }
    if let Some(ref partition) = options.partition_filter {
        InputValidator::validate_partition_list(partition).map_err(|e| anyhow::anyhow!("{}", e))?;
    }
    if let Some(ref jobs) = options.jobs {
        InputValidator::validate_job_ids(jobs).map_err(|e| anyhow::anyhow!("{}", e))?;
    }
    if let Some(ref start) = options.starttime {
        InputValidator::validate_time_string(start).map_err(|e| anyhow::anyhow!("{}", e))?;
    }
    if let Some(ref end) = options.endtime {
        InputValidator::validate_time_string(end).map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    // Show default message if no options specified
    if options.starttime.is_none()
        && options.partition_filter.is_none()
        && options.user.is_none()
        && !options.allusers
        && options.jobs.is_none()
    {
        eprintln!("No sacct options specified.");
        eprintln!("Using default: all users from last 24 hours");
    }

    let sacct_output = run_sacct(
        options.starttime.as_deref(),
        options.endtime.as_deref(),
        options.partition_filter.as_deref(),
        options.account_filter.as_deref(),
        options.user.as_deref(),
        options.allusers,
        options.jobs.as_deref(),
        options.user_specified_time,
        options.debug,
    )
    .context("Failed to run sacct")?;

    let all_jobs = SlurmJobParser::parse_string(&sacct_output)
        .map_err(|e| anyhow::anyhow!("Failed to parse sacct output: {}", e))?;

    if all_jobs.is_empty() {
        let mut hints = Vec::new();
        if options.starttime.is_some() || options.endtime.is_some() {
            hints.push("Try a wider time range (e.g., --starttime=2025-01-01)");
        } else {
            hints.push("Try specifying --starttime (default is last 24 hours)");
        }
        if options.partition_filter.is_some() {
            hints.push("Remove --partition to search all partitions");
        }
        if options.user.is_some() {
            hints.push("Remove -u to search all users, or use -a/--allusers");
        }
        if options.jobs.is_some() {
            hints.push("Verify the job IDs exist with: sacct -j <id>");
        }
        let suggestion = if hints.is_empty() {
            String::new()
        } else {
            format!("\nSuggestions:\n  {}", hints.join("\n  "))
        };
        bail!("No valid jobs found.{}", suggestion);
    }

    if options.debug {
        eprintln!("Debug: Parsed {} jobs from sacct output", all_jobs.len());
    }

    Ok(all_jobs)
}

/// Calculate efficiency metrics for all jobs.
pub fn calculate_metrics_for_jobs(jobs: &[SlurmJob], debug: bool) -> Vec<GPUMetrics> {
    if debug {
        eprintln!("Debug: Building node-to-GPU mapping...");
    }
    let _node_gpu_map = build_node_gpu_mapping(debug);

    if debug {
        eprintln!("Debug: Getting partition time limits...");
    }
    let partition_limits = get_partition_time_limits(debug);

    let mut metrics = Vec::with_capacity(jobs.len());
    let failed_jobs = 0;

    for (i, job) in jobs.iter().enumerate() {
        let metric = EfficiencyCalculator::calculate_metrics(job, partition_limits, debug);
        metrics.push(metric);

        if debug && i < 3 {
            let m = &metrics[metrics.len() - 1];
            eprintln!(
                "Debug: Job {} - State: {}, GPU Eff: {}, GPU Mem: {}",
                job.job_id, m.state, m.gpu_eff, m.gpu_mem_eff
            );
        }
    }

    if debug {
        eprintln!(
            "Debug: Successfully calculated metrics for {} jobs",
            metrics.len()
        );
        if failed_jobs > 0 {
            eprintln!(
                "Debug: Failed to calculate metrics for {} jobs",
                failed_jobs
            );
        }
    }

    metrics
}

/// Filter for GPU jobs only.
pub fn filter_gpu_jobs(jobs: Vec<SlurmJob>, debug: bool) -> Vec<SlurmJob> {
    let original_count = jobs.len();
    let gpu_jobs: Vec<SlurmJob> = jobs
        .into_iter()
        .filter(EfficiencyCalculator::job_requests_gpu)
        .collect();

    if debug {
        eprintln!(
            "Debug: Filtered {} jobs to {} GPU jobs",
            original_count,
            gpu_jobs.len()
        );
    }

    if gpu_jobs.is_empty() {
        eprintln!("No jobs requesting GPU resources found.");
        eprintln!("  Hint: Remove --gpu to see all jobs, or try a wider time range.");
    }

    gpu_jobs
}

/// Apply all filters to the metrics list.
pub fn filter_metrics(metrics: Vec<GPUMetrics>, options: &ReportOptions) -> Vec<GPUMetrics> {
    let mut metrics = metrics;

    // Filter by state
    if options.filter_state != "all" {
        let lower = options.filter_state.to_lowercase();
        let target_state = match lower.as_str() {
            "completed" => "COMPLETED",
            "failed" => "FAILED",
            "pending" => "PENDING",
            "running" => "RUNNING",
            "timeout" => "TIMEOUT",
            "cancelled" | "canceled" => "CANCELLED",
            "out_of_memory" | "oom" => "OUT_OF_MEMORY",
            "node_fail" | "nodefail" => "NODE_FAIL",
            "preempted" => "PREEMPTED",
            other => other,
        };
        metrics.retain(|m| m.state.matches_filter(target_state));
    }

    // Filter by minimum GPU efficiency
    if let Some(min_eff) = options.min_gpu_eff {
        metrics.retain(|m| {
            if m.gpu_eff == "---" {
                false
            } else {
                m.gpu_eff
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .is_ok_and(|v| v >= min_eff)
            }
        });
    }

    // Filter for GPU idle
    if options.gpu_idle {
        metrics.retain(|m| {
            if m.gpu_count == 0 {
                return false;
            }

            let gpu_util_low = if m.gpu_util != "---" {
                m.gpu_util
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .is_ok_and(|v| v < GPU_IDLE_THRESHOLD_PERCENT)
            } else {
                false
            };

            let gpu_mem_eff_low = if m.gpu_mem_eff != "---" {
                m.gpu_mem_eff
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .is_ok_and(|v| v < GPU_MEMORY_IDLE_THRESHOLD_PERCENT)
            } else {
                false
            };

            gpu_util_low || gpu_mem_eff_low
        });
    }

    if metrics.is_empty() {
        let mut hints = Vec::new();
        if options.filter_state != "all" {
            hints.push(format!(
                "Remove --filter-state (currently '{}')",
                options.filter_state
            ));
        }
        if options.min_gpu_eff.is_some() {
            hints.push("Lower or remove --min-gpu-eff threshold".to_string());
        }
        if options.gpu_idle {
            hints.push("Remove --gpu-idle to see all utilization levels".to_string());
        }
        eprintln!("No jobs match the specified filters.");
        for hint in &hints {
            eprintln!("  Hint: {}", hint);
        }
    }

    // Limit number of jobs
    if let Some(max_jobs) = options.max_jobs {
        metrics.truncate(max_jobs);
    }

    metrics
}

/// Generate and output summary report.
pub fn generate_and_output_summary(metrics: &[GPUMetrics], options: &ReportOptions) -> Result<()> {
    let account_only = options.summary_by_account && !options.allusers;

    let mut summaries = EfficiencyCalculator::calculate_summary_metrics(
        metrics,
        options.summary_by_partition,
        options.summary_by_account && options.allusers,
        account_only,
    );

    sort_summary_metrics(&mut summaries, &options.sort_by, options.reverse);

    if let Some(ref output_path) = options.output {
        let report = GPUReporter::format_summary_report(
            &summaries,
            options.summary_by_partition,
            options.summary_by_account && options.allusers,
            account_only,
        );
        std::fs::write(output_path, &report)
            .with_context(|| format!("Failed to write summary report to {}", output_path))?;
        eprintln!("Summary report written to {}", output_path);
    } else {
        GPUReporter::print_summary_report(
            &summaries,
            !options.plain,
            options.summary_by_partition,
            options.summary_by_account && options.allusers,
            account_only,
        );
    }

    Ok(())
}

/// Generate and output detailed report.
pub fn generate_and_output_report(metrics: &[GPUMetrics], options: &ReportOptions) -> Result<()> {
    let show_weighted_avg = options.user.is_some();

    // Telegraf output path
    if options.telegraf {
        // Only include terminal jobs — running/pending have no complete efficiency data
        // and would emit misleading zeros.
        let terminal: Vec<&GPUMetrics> = metrics.iter().filter(|m| m.state.is_terminal()).collect();
        if terminal.is_empty() {
            return Ok(());
        }

        if options.allusers {
            // Group metrics by user, output one line per user
            let mut by_user: HashMap<String, Vec<&GPUMetrics>> = HashMap::new();
            for m in terminal {
                by_user.entry(m.user.clone()).or_default().push(m);
            }
            let mut users: Vec<&String> = by_user.keys().collect();
            users.sort();
            for user in users {
                let user_metrics: Vec<GPUMetrics> =
                    by_user[user].iter().map(|m| (*m).clone()).collect();
                let line = GPUReporter::format_telegraf_weighted_avg(
                    &user_metrics,
                    user,
                    options.partition_filter.as_deref(),
                    options.account_filter.as_deref(),
                );
                if !line.is_empty() {
                    println!("{}", line);
                }
            }
        } else {
            let terminal_owned: Vec<GPUMetrics> = terminal.iter().map(|m| (*m).clone()).collect();
            let user = options
                .user
                .as_deref()
                .or_else(|| terminal_owned.first().map(|m| m.user.as_str()))
                .unwrap_or("unknown");
            let line = GPUReporter::format_telegraf_weighted_avg(
                &terminal_owned,
                user,
                options.partition_filter.as_deref(),
                options.account_filter.as_deref(),
            );
            if !line.is_empty() {
                println!("{}", line);
            }
        }
        return Ok(());
    }

    if let Some(ref output_path) = options.output {
        let report = GPUReporter::format_report(
            metrics,
            options.show_partition,
            options.detailed,
            show_weighted_avg,
        );
        std::fs::write(output_path, &report)
            .with_context(|| format!("Failed to write report to {}", output_path))?;
        eprintln!("Report written to {}", output_path);
    } else {
        GPUReporter::print_report(
            metrics,
            !options.plain,
            options.show_partition,
            options.detailed,
            show_weighted_avg,
        );
    }

    Ok(())
}
