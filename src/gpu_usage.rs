use std::collections::{HashMap, HashSet};

use comfy_table::{Attribute, Cell, CellAlignment, Color, Table};

use crate::models::*;
use crate::queued_jobs::QueuedJobsCollector;
use crate::table_helpers::*;
use crate::resource_slots::ResourceSlotCollector;
use crate::resource_summary::ResourceSummarizer;
use crate::tres_parser::TresParser;
use crate::user_usage::{UserResourceUsage, UserUsageAnalyzer};

/// Facade for GPU usage calculations, delegating to specialized modules.
pub struct GPUUsageCalculator;

impl GPUUsageCalculator {
    pub fn collect_gpu_slots_info(
        debug: bool,
        partitions: Option<&[String]>,
    ) -> Vec<GPUSlotsInfo> {
        ResourceSlotCollector::collect_gpu_slots_info(debug, partitions)
    }

    pub fn collect_cpu_slots_info(
        debug: bool,
        partitions: Option<&[String]>,
    ) -> Vec<CPUSlotsInfo> {
        ResourceSlotCollector::collect_cpu_slots_info(debug, partitions)
    }

    pub fn summarize_by_type(gpu_slots_list: &[GPUSlotsInfo]) -> Vec<GPUTypeSummary> {
        ResourceSummarizer::summarize_by_gpu_type(gpu_slots_list)
    }

    pub fn summarize_cpu_usage(cpu_slots_list: &[CPUSlotsInfo]) -> CPUTypeSummary {
        ResourceSummarizer::summarize_cpu_usage(cpu_slots_list)
    }

    pub fn get_user_gpu_usage(user: &str, debug: bool) -> HashMap<String, (i32, usize)> {
        UserUsageAnalyzer::get_user_gpu_usage(user, debug)
    }

    pub fn get_user_cpu_usage(user: &str, debug: bool) -> i32 {
        UserUsageAnalyzer::get_user_cpu_usage(user, debug)
    }

    pub fn get_all_users_resource_usage(
        debug: bool,
        partitions: Option<&[String]>,
    ) -> UserResourceUsage {
        UserUsageAnalyzer::get_all_users_resource_usage(debug, partitions)
    }

    pub fn get_queued_jobs_info(
        debug: bool,
        partitions: Option<&[String]>,
    ) -> Vec<QueuedJobInfo> {
        QueuedJobsCollector::get_queued_jobs_info(debug, partitions)
    }

    pub fn get_queued_jobs_by_user(
        debug: bool,
        partitions: Option<&[String]>,
    ) -> HashMap<String, usize> {
        QueuedJobsCollector::get_queued_jobs_by_user(debug, partitions)
    }

    pub fn summarize_queue_info(queued_jobs: &[QueuedJobInfo]) -> QueueSummary {
        ResourceSummarizer::summarize_queue_info(queued_jobs)
    }
}

/// Reports current GPU usage by type.
pub struct GPUUsageReporter;

impl GPUUsageReporter {
    /// Format a plain text report for user-specific GPU usage.
    pub fn format_user_usage_report(
        user_gpu_data: &HashMap<String, (i32, usize)>,
        user: &str,
        user_cpu_count: i32,
        queued_cpus: i32,
        queued_gpus: i32,
        queued_job_count: usize,
    ) -> String {
        let mut lines = Vec::new();
        lines.push(format!("GPU Usage for User: {}", user));
        lines.push("GPU Type     GPUs Used  CPUs Used  Nodes".to_string());
        lines.push("-".repeat(50));

        let total_gpus: i32 = user_gpu_data.values().map(|(c, _)| c).sum();

        let mut sorted_data: Vec<_> = user_gpu_data.iter().collect();
        sorted_data.sort_by_key(|(k, _)| (*k).clone());

        for (gpu_type, (gpu_count, node_count)) in &sorted_data {
            let cpu_display = if user_cpu_count > 0 && total_gpus > 0 {
                if user_gpu_data.len() == 1 {
                    format!("{}", user_cpu_count)
                } else {
                    let proportional_cpus =
                        ((*gpu_count as f64 / total_gpus as f64) * user_cpu_count as f64).round()
                            as i32;
                    if proportional_cpus > 0 {
                        format!("{}", proportional_cpus)
                    } else {
                        String::new()
                    }
                }
            } else {
                String::new()
            };

            lines.push(format!(
                "{:<12} {:>9} {:>10} {:>6}",
                gpu_type, gpu_count, cpu_display, node_count
            ));
        }

        lines.push("-".repeat(50));
        lines.push(format!(
            "{:<12} {:>9} {:>10}",
            "TOTAL", total_gpus, user_cpu_count
        ));

        if queued_job_count > 0 {
            lines.push("-".repeat(50));
            let tasks_text = if queued_job_count != 1 {
                format!("{} tasks", queued_job_count)
            } else {
                "1 task".to_string()
            };
            lines.push(format!(
                "{:<12} {:>9} {:>10} {:>6}",
                "QUEUED", queued_gpus, queued_cpus, tasks_text
            ));
        }

        lines.join("\n")
    }

    /// Format a rich table for user-specific GPU usage.
    pub fn format_rich_user_usage_report(
        user_gpu_data: &HashMap<String, (i32, usize)>,
        _user: &str,
        user_cpu_count: i32,
        queued_cpus: i32,
        queued_gpus: i32,
        queued_job_count: usize,
    ) -> Table {
        let mut table = new_table();

        table.set_header(vec![
            hdr("GPU Type"),
            hdr_right("GPUs Used"),
            hdr_right("CPUs Used"),
            hdr_right("Nodes"),
        ]);

        let total_gpus: i32 = user_gpu_data.values().map(|(c, _)| c).sum();

        let mut sorted_data: Vec<_> = user_gpu_data.iter().collect();
        sorted_data.sort_by_key(|(k, _)| (*k).clone());

        for (gpu_type, (gpu_count, node_count)) in &sorted_data {
            let cpu_display = if user_cpu_count > 0 && total_gpus > 0 {
                if user_gpu_data.len() == 1 {
                    format!("{}", user_cpu_count)
                } else {
                    let proportional_cpus =
                        ((*gpu_count as f64 / total_gpus as f64) * user_cpu_count as f64).round()
                            as i32;
                    if proportional_cpus > 0 {
                        format!("{}", proportional_cpus)
                    } else {
                        String::new()
                    }
                }
            } else {
                String::new()
            };

            table.add_row(vec![
                Cell::new(gpu_type).fg(Color::Cyan),
                Cell::new(gpu_count.to_string()).set_alignment(CellAlignment::Right),
                Cell::new(&cpu_display).set_alignment(CellAlignment::Right),
                Cell::new(node_count.to_string()).set_alignment(CellAlignment::Right),
            ]);
        }

        // Total row
        table.add_row(vec![
            Cell::new("TOTAL").add_attribute(Attribute::Bold),
            Cell::new(total_gpus.to_string()).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new(user_cpu_count.to_string()).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new(""),
        ]);

        // Queued row
        if queued_job_count > 0 {
            let tasks_text = if queued_job_count != 1 {
                format!("{} tasks", queued_job_count)
            } else {
                "1 task".to_string()
            };
            table.add_row(vec![
                Cell::new("QUEUED")
                    .add_attribute(Attribute::Bold)
                    .fg(Color::Yellow),
                Cell::new(queued_gpus.to_string())
                    .add_attribute(Attribute::Bold)
                    .fg(Color::Yellow)
                    .set_alignment(CellAlignment::Right),
                Cell::new(queued_cpus.to_string())
                    .add_attribute(Attribute::Bold)
                    .fg(Color::Yellow)
                    .set_alignment(CellAlignment::Right),
                Cell::new(&tasks_text)
                    .add_attribute(Attribute::Bold)
                    .fg(Color::Yellow)
                    .set_alignment(CellAlignment::Right),
            ]);
        }

        table
    }

    /// Format cluster resource report as rich table.
    pub fn format_cluster_report(
        gpu_summaries: &[GPUTypeSummary],
        cpu_summary: &CPUTypeSummary,
        queue_summary: &QueueSummary,
        plain: bool,
    ) -> String {
        if plain {
            Self::format_cluster_report_plain(gpu_summaries, cpu_summary, queue_summary)
        } else {
            let table =
                Self::format_cluster_report_rich(gpu_summaries, cpu_summary, queue_summary);
            format!("{}", table)
        }
    }

    fn format_cluster_report_rich(
        gpu_summaries: &[GPUTypeSummary],
        cpu_summary: &CPUTypeSummary,
        queue_summary: &QueueSummary,
    ) -> Table {
        let mut table = new_table();

        table.set_header(vec![
            hdr("Resource Type"),
            hdr_right("Total"),
            hdr_right("Used"),
            hdr_right("Available"),
            hdr_right("Utilization"),
            hdr("Details"),
        ]);

        // Add GPU rows
        for summary in gpu_summaries {
            let util_color = utilization_color(summary.utilization_percent());
            table.add_row(vec![
                Cell::new(format!("GPUs ({})", summary.gpu_type)).fg(Color::Cyan),
                Cell::new(summary.total_gpus.to_string()).set_alignment(CellAlignment::Right),
                Cell::new(summary.used_gpus.to_string()).set_alignment(CellAlignment::Right),
                Cell::new(summary.available_gpus.to_string()).set_alignment(CellAlignment::Right),
                Cell::new(format!("{:.1}%", summary.utilization_percent()))
                    .fg(util_color)
                    .set_alignment(CellAlignment::Right),
                Cell::new(format!("{} nodes", summary.nodes_with_type.len())),
            ]);
        }

        // GPU total
        let total_gpus: i32 = gpu_summaries.iter().map(|s| s.total_gpus).sum();
        let used_gpus: i32 = gpu_summaries.iter().map(|s| s.used_gpus).sum();
        let gpu_util = if total_gpus > 0 {
            used_gpus as f64 / total_gpus as f64 * 100.0
        } else {
            0.0
        };
        let total_nodes: usize = gpu_summaries
            .iter()
            .flat_map(|s| s.nodes_with_type.iter())
            .collect::<HashSet<_>>()
            .len();
        let gpu_util_color = utilization_color(gpu_util);

        table.add_row(vec![
            Cell::new("GPUs (TOTAL)").add_attribute(Attribute::Bold),
            Cell::new(total_gpus.to_string()).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new(used_gpus.to_string()).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new((total_gpus - used_gpus).to_string()).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new(format!("{:.1}%", gpu_util))
                .add_attribute(Attribute::Bold)
                .fg(gpu_util_color)
                .set_alignment(CellAlignment::Right),
            Cell::new(format!("{} nodes", total_nodes)).add_attribute(Attribute::Bold),
        ]);

        // CPU row
        let cpu_util_color = utilization_color(cpu_summary.utilization_percent());
        table.add_row(vec![
            Cell::new("CPUs (TOTAL)")
                .add_attribute(Attribute::Bold)
                .fg(Color::Yellow),
            Cell::new(format!("{}", cpu_summary.total_cpus)).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new(format!("{}", cpu_summary.used_cpus)).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new(format!("{}", cpu_summary.available_cpus)).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new(format!("{:.1}%", cpu_summary.utilization_percent()))
                .add_attribute(Attribute::Bold)
                .fg(cpu_util_color)
                .set_alignment(CellAlignment::Right),
            Cell::new(format!("{} nodes", cpu_summary.nodes_with_cpus.len()))
                .add_attribute(Attribute::Bold),
        ]);

        // Queue row
        if queue_summary.total_jobs > 0 {
            table.add_row(vec![
                Cell::new("Queued Jobs")
                    .add_attribute(Attribute::Bold)
                    .fg(Color::Yellow),
                Cell::new(queue_summary.total_jobs.to_string())
                    .add_attribute(Attribute::Bold)
                    .set_alignment(CellAlignment::Right),
                Cell::new("-").set_alignment(CellAlignment::Right),
                Cell::new("-").set_alignment(CellAlignment::Right),
                Cell::new("-").set_alignment(CellAlignment::Right),
                Cell::new(format!(
                    "{} CPUs, {} GPUs",
                    queue_summary.total_cpus, queue_summary.total_gpus
                ))
                .add_attribute(Attribute::Bold),
            ]);
        }

        table
    }

    fn format_cluster_report_plain(
        gpu_summaries: &[GPUTypeSummary],
        cpu_summary: &CPUTypeSummary,
        queue_summary: &QueueSummary,
    ) -> String {
        let mut lines = Vec::new();
        lines.push("Cluster Resource Usage".to_string());
        lines.push("=".repeat(80));
        lines.push(format!(
            "{:<15} {:>8} {:>8} {:>10} {:>12} {:<20}",
            "Resource Type", "Total", "Used", "Available", "Utilization", "Details"
        ));
        lines.push("-".repeat(80));

        for summary in gpu_summaries {
            let label = format!("GPUs ({})", summary.gpu_type);
            lines.push(format!(
                "{:<15} {:>8} {:>8} {:>10} {:>11.1}% {} nodes",
                &label[..label.len().min(14)],
                summary.total_gpus,
                summary.used_gpus,
                summary.available_gpus,
                summary.utilization_percent(),
                summary.nodes_with_type.len()
            ));
        }

        let total_gpus: i32 = gpu_summaries.iter().map(|s| s.total_gpus).sum();
        let used_gpus: i32 = gpu_summaries.iter().map(|s| s.used_gpus).sum();
        let gpu_util = if total_gpus > 0 {
            used_gpus as f64 / total_gpus as f64 * 100.0
        } else {
            0.0
        };
        let total_nodes: usize = gpu_summaries
            .iter()
            .flat_map(|s| s.nodes_with_type.iter())
            .collect::<HashSet<_>>()
            .len();

        lines.push(format!(
            "{:<15} {:>8} {:>8} {:>10} {:>11.1}% {} nodes",
            "GPUs (TOTAL)",
            total_gpus,
            used_gpus,
            total_gpus - used_gpus,
            gpu_util,
            total_nodes
        ));

        lines.push(format!(
            "{:<15} {:>8} {:>8} {:>10} {:>11.1}% {} nodes",
            "CPUs (TOTAL)",
            cpu_summary.total_cpus,
            cpu_summary.used_cpus,
            cpu_summary.available_cpus,
            cpu_summary.utilization_percent(),
            cpu_summary.nodes_with_cpus.len()
        ));

        if queue_summary.total_jobs > 0 {
            lines.push(format!(
                "{:<15} {:>8} {:>8} {:>10} {:>12} {} CPUs, {} GPUs",
                "Queued Jobs",
                queue_summary.total_jobs,
                "-",
                "-",
                "-",
                queue_summary.total_cpus,
                queue_summary.total_gpus
            ));
        }

        lines.join("\n")
    }

    /// Format all-users resource usage report.
    pub fn format_all_users_report(
        all_users_gpu_data: &HashMap<String, HashMap<String, i32>>,
        all_users_cpu_data: &HashMap<String, i32>,
        queued_jobs_by_user: &HashMap<String, usize>,
        all_users_memory_data: &HashMap<String, i64>,
        plain: bool,
    ) -> String {
        if plain {
            Self::format_all_users_report_plain(
                all_users_gpu_data,
                all_users_cpu_data,
                queued_jobs_by_user,
                all_users_memory_data,
            )
        } else {
            let table = Self::format_all_users_report_rich(
                all_users_gpu_data,
                all_users_cpu_data,
                queued_jobs_by_user,
                all_users_memory_data,
            );
            format!("{}", table)
        }
    }

    fn format_all_users_report_rich(
        all_users_gpu_data: &HashMap<String, HashMap<String, i32>>,
        all_users_cpu_data: &HashMap<String, i32>,
        queued_jobs_by_user: &HashMap<String, usize>,
        all_users_memory_data: &HashMap<String, i64>,
    ) -> Table {
        let mut table = new_table();

        table.set_header(vec![
            hdr("User"),
            hdr("GPU Type"),
            hdr_right("GPUs Used"),
            hdr_right("CPUs Used"),
            hdr_right("Memory"),
            hdr_right("Queued"),
        ]);

        // Collect and sort users
        let mut all_users: HashSet<&String> = HashSet::new();
        all_users.extend(all_users_gpu_data.keys());
        all_users.extend(all_users_cpu_data.keys());
        all_users.extend(queued_jobs_by_user.keys());
        all_users.extend(all_users_memory_data.keys());

        let mut user_totals: Vec<(&String, i32, i32, usize)> = all_users
            .iter()
            .map(|user| {
                let total_gpus: i32 = all_users_gpu_data
                    .get(*user)
                    .map(|m| m.values().sum())
                    .unwrap_or(0);
                let total_cpus = *all_users_cpu_data.get(*user).unwrap_or(&0);
                let queued_count = *queued_jobs_by_user.get(*user).unwrap_or(&0);
                (*user, total_gpus, total_cpus, queued_count)
            })
            .collect();
        user_totals.sort_by(|a, b| {
            let weight_a = a.1 as i64 * 100 + a.2 as i64 + a.3 as i64 * 10;
            let weight_b = b.1 as i64 * 100 + b.2 as i64 + b.3 as i64 * 10;
            weight_b.cmp(&weight_a)
        });

        let mut grand_total_gpus = 0;
        let mut grand_total_cpus = 0;
        let mut grand_total_memory: i64 = 0;
        let mut grand_total_queued: usize = 0;

        for (user, _, _, queued_count) in &user_totals {
            let gpu_data = all_users_gpu_data.get(*user);
            let user_cpu_count = *all_users_cpu_data.get(*user).unwrap_or(&0);
            let user_memory_mb = *all_users_memory_data.get(*user).unwrap_or(&0);
            let total_user_gpus: i32 = gpu_data.map(|m| m.values().sum()).unwrap_or(0);

            grand_total_cpus += user_cpu_count;
            grand_total_memory += user_memory_mb;

            let mut first_row = true;

            if let Some(gpu_map) = gpu_data {
                let mut gpu_entries: Vec<_> = gpu_map.iter().collect();
                gpu_entries.sort_by_key(|(k, _)| (*k).clone());

                for (gpu_type, gpu_count) in &gpu_entries {
                    grand_total_gpus += *gpu_count;

                    let cpu_display = if user_cpu_count > 0 && total_user_gpus > 0 {
                        if gpu_map.len() == 1 {
                            format!("{}", user_cpu_count)
                        } else {
                            let proportional =
                                ((**gpu_count as f64 / total_user_gpus as f64) * user_cpu_count as f64)
                                    .round() as i32;
                            if proportional > 0 {
                                format!("{}", proportional)
                            } else {
                                String::new()
                            }
                        }
                    } else {
                        String::new()
                    };

                    let memory_display = if first_row {
                        TresParser::format_memory_value(user_memory_mb as f64)
                    } else {
                        String::new()
                    };

                    let queue_display = if first_row && *queued_count > 0 {
                        grand_total_queued += *queued_count;
                        format!("{} tasks", queued_count)
                    } else if !first_row {
                        String::new()
                    } else {
                        "-".to_string()
                    };

                    table.add_row(vec![
                        Cell::new(if first_row { user.as_str() } else { "" }).fg(Color::Cyan),
                        Cell::new(*gpu_type).fg(Color::Yellow),
                        Cell::new(gpu_count.to_string()).set_alignment(CellAlignment::Right),
                        Cell::new(&cpu_display).set_alignment(CellAlignment::Right),
                        Cell::new(&memory_display).set_alignment(CellAlignment::Right),
                        Cell::new(&queue_display).set_alignment(CellAlignment::Right),
                    ]);
                    first_row = false;
                }
            }

            if gpu_data.map_or(true, |m| m.is_empty())
                && (user_cpu_count > 0 || user_memory_mb > 0 || *queued_count > 0)
            {
                let cpu_display = if user_cpu_count > 0 {
                    format!("{}", user_cpu_count)
                } else {
                    String::new()
                };
                let memory_display = TresParser::format_memory_value(user_memory_mb as f64);
                let queue_display = if *queued_count > 0 {
                    grand_total_queued += *queued_count;
                    format!("{} tasks", queued_count)
                } else {
                    "-".to_string()
                };

                table.add_row(vec![
                    Cell::new(user.as_str()).fg(Color::Cyan),
                    Cell::new("No GPUs"),
                    Cell::new("0").set_alignment(CellAlignment::Right),
                    Cell::new(&cpu_display).set_alignment(CellAlignment::Right),
                    Cell::new(&memory_display).set_alignment(CellAlignment::Right),
                    Cell::new(&queue_display).set_alignment(CellAlignment::Right),
                ]);
            }
        }

        // Total row
        table.add_row(vec![
            Cell::new("TOTAL").add_attribute(Attribute::Bold),
            Cell::new(""),
            Cell::new(grand_total_gpus.to_string()).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new(grand_total_cpus.to_string()).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new(TresParser::format_memory_value(grand_total_memory as f64))
                .add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            Cell::new(format!("{} tasks", grand_total_queued)).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
        ]);

        table
    }

    fn format_all_users_report_plain(
        all_users_gpu_data: &HashMap<String, HashMap<String, i32>>,
        all_users_cpu_data: &HashMap<String, i32>,
        queued_jobs_by_user: &HashMap<String, usize>,
        all_users_memory_data: &HashMap<String, i64>,
    ) -> String {
        let mut lines = Vec::new();
        lines.push("Resource Usage by User".to_string());
        lines.push("=".repeat(82));
        lines.push(format!(
            "{:<15} {:<12} {:>10} {:>10} {:>10} {:>12}",
            "User", "GPU Type", "GPUs Used", "CPUs Used", "Memory", "Queued"
        ));
        lines.push("-".repeat(82));

        let mut all_users: HashSet<&String> = HashSet::new();
        all_users.extend(all_users_gpu_data.keys());
        all_users.extend(all_users_cpu_data.keys());
        all_users.extend(queued_jobs_by_user.keys());
        all_users.extend(all_users_memory_data.keys());

        let mut user_totals: Vec<(&String, i32, i32, usize)> = all_users
            .iter()
            .map(|user| {
                let total_gpus: i32 = all_users_gpu_data
                    .get(*user)
                    .map(|m| m.values().sum())
                    .unwrap_or(0);
                let total_cpus = *all_users_cpu_data.get(*user).unwrap_or(&0);
                let queued_count = *queued_jobs_by_user.get(*user).unwrap_or(&0);
                (*user, total_gpus, total_cpus, queued_count)
            })
            .collect();
        user_totals.sort_by(|a, b| {
            let weight_a = a.1 as i64 * 100 + a.2 as i64 + a.3 as i64 * 10;
            let weight_b = b.1 as i64 * 100 + b.2 as i64 + b.3 as i64 * 10;
            weight_b.cmp(&weight_a)
        });

        let mut grand_total_gpus = 0;
        let mut grand_total_cpus = 0;
        let mut grand_total_memory: i64 = 0;
        let mut grand_total_queued: usize = 0;

        for (user, _, _, queued_count) in &user_totals {
            let gpu_data = all_users_gpu_data.get(*user);
            let user_cpu_count = *all_users_cpu_data.get(*user).unwrap_or(&0);
            let user_memory_mb = *all_users_memory_data.get(*user).unwrap_or(&0);
            let total_user_gpus: i32 = gpu_data.map(|m| m.values().sum()).unwrap_or(0);

            grand_total_cpus += user_cpu_count;
            grand_total_memory += user_memory_mb;

            let mut first_row = true;

            if let Some(gpu_map) = gpu_data {
                let mut gpu_entries: Vec<_> = gpu_map.iter().collect();
                gpu_entries.sort_by_key(|(k, _)| (*k).clone());

                for (gpu_type, gpu_count) in &gpu_entries {
                    grand_total_gpus += *gpu_count;

                    let cpu_display = if user_cpu_count > 0 && total_user_gpus > 0 {
                        if gpu_map.len() == 1 {
                            format!("{}", user_cpu_count)
                        } else {
                            let proportional =
                                ((**gpu_count as f64 / total_user_gpus as f64)
                                    * user_cpu_count as f64)
                                    .round() as i32;
                            if proportional > 0 {
                                format!("{}", proportional)
                            } else {
                                String::new()
                            }
                        }
                    } else {
                        String::new()
                    };

                    let memory_display = if first_row {
                        TresParser::format_memory_value(user_memory_mb as f64)
                    } else {
                        String::new()
                    };

                    let queue_display = if first_row && *queued_count > 0 {
                        grand_total_queued += *queued_count;
                        format!("{} tasks", queued_count)
                    } else if !first_row {
                        String::new()
                    } else {
                        "-".to_string()
                    };

                    lines.push(format!(
                        "{:<15} {:<12} {:>10} {:>10} {:>10} {:>12}",
                        if first_row { user.as_str() } else { "" },
                        gpu_type,
                        gpu_count,
                        cpu_display,
                        memory_display,
                        queue_display
                    ));
                    first_row = false;
                }
            }

            if gpu_data.map_or(true, |m| m.is_empty())
                && (user_cpu_count > 0 || user_memory_mb > 0 || *queued_count > 0)
            {
                let cpu_display = if user_cpu_count > 0 {
                    format!("{}", user_cpu_count)
                } else {
                    String::new()
                };
                let memory_display = TresParser::format_memory_value(user_memory_mb as f64);
                let queue_display = if *queued_count > 0 {
                    grand_total_queued += *queued_count;
                    format!("{} tasks", queued_count)
                } else {
                    "-".to_string()
                };

                lines.push(format!(
                    "{:<15} {:<12} {:>10} {:>10} {:>10} {:>12}",
                    user, "No GPUs", "0", cpu_display, memory_display, queue_display
                ));
            }
        }

        lines.push("-".repeat(82));
        lines.push(format!(
            "{:<15} {:<12} {:>10} {:>10} {:>10} {} tasks",
            "TOTAL",
            "",
            grand_total_gpus,
            grand_total_cpus,
            TresParser::format_memory_value(grand_total_memory as f64),
            grand_total_queued
        ));

        lines.join("\n")
    }

    /// Print the GPU usage report.
    pub fn print_usage_report(
        use_rich: bool,
        detailed: bool,
        debug: bool,
        partitions: Option<&[String]>,
        user: Option<&str>,
        all_users: bool,
        telegraf: bool,
    ) {
        // Telegraf format doesn't support user-specific or all-users views
        if telegraf && (user.is_some() || all_users) {
            eprintln!("Warning: --telegraf format only supports cluster-wide GPU usage. Ignoring --user and --all options.");
        }

        if let Some(user) = user {
            if debug {
                eprintln!("Debug: Getting GPU usage for user: {}", user);
            }

            let user_gpu_usage = GPUUsageCalculator::get_user_gpu_usage(user, debug);
            let user_cpu_usage = GPUUsageCalculator::get_user_cpu_usage(user, debug);

            let all_queued_jobs = GPUUsageCalculator::get_queued_jobs_info(debug, partitions);
            let user_queued_jobs: Vec<_> =
                all_queued_jobs.iter().filter(|j| j.user == user).collect();

            let queued_cpus: i32 = user_queued_jobs.iter().map(|j| j.cpu_request).sum();
            let queued_gpus: i32 = user_queued_jobs.iter().map(|j| j.gpu_request).sum();
            let queued_job_count = user_queued_jobs.len();

            if user_gpu_usage.is_empty() && queued_job_count == 0 {
                eprintln!(
                    "User {} has no running GPU jobs or queued jobs.",
                    user
                );
                return;
            }

            if use_rich && !detailed {
                let table = Self::format_rich_user_usage_report(
                    &user_gpu_usage,
                    user,
                    user_cpu_usage,
                    queued_cpus,
                    queued_gpus,
                    queued_job_count,
                );
                println!("{}", table);
            } else {
                let report = Self::format_user_usage_report(
                    &user_gpu_usage,
                    user,
                    user_cpu_usage,
                    queued_cpus,
                    queued_gpus,
                    queued_job_count,
                );
                println!("{}", report);
            }
        } else if all_users && !telegraf {
            if debug {
                eprintln!("Debug: Getting enhanced usage data for all users");
                if let Some(parts) = partitions {
                    eprintln!("Debug: Filtering by partitions: {}", parts.join(", "));
                }
            }

            let resource_usage =
                GPUUsageCalculator::get_all_users_resource_usage(debug, partitions);
            let queued_jobs_by_user = GPUUsageCalculator::get_queued_jobs_by_user(debug, partitions);

            if resource_usage.gpu_usage.is_empty()
                && resource_usage.cpu_usage.is_empty()
                && queued_jobs_by_user.is_empty()
            {
                eprintln!("No users with running jobs or queued jobs found.");
                return;
            }

            let report = Self::format_all_users_report(
                &resource_usage.gpu_usage,
                &resource_usage.cpu_usage,
                &queued_jobs_by_user,
                &resource_usage.memory_usage,
                !use_rich,
            );
            println!("{}", report);
        } else {
            if debug {
                eprintln!("Debug: Getting enhanced cluster resource data");
            }

            let gpu_slots_list = GPUUsageCalculator::collect_gpu_slots_info(debug, partitions);
            let cpu_slots_list = GPUUsageCalculator::collect_cpu_slots_info(debug, partitions);
            let queued_jobs = GPUUsageCalculator::get_queued_jobs_info(debug, partitions);

            if gpu_slots_list.is_empty() && cpu_slots_list.is_empty() {
                if let Some(parts) = partitions {
                    eprintln!(
                        "No nodes found in partition(s): {}.",
                        parts.join(", ")
                    );
                } else {
                    eprintln!("No nodes found.");
                }
                return;
            }

            let gpu_summaries = if gpu_slots_list.is_empty() {
                Vec::new()
            } else {
                GPUUsageCalculator::summarize_by_type(&gpu_slots_list)
            };
            let cpu_summary = GPUUsageCalculator::summarize_cpu_usage(&cpu_slots_list);
            let queue_summary = GPUUsageCalculator::summarize_queue_info(&queued_jobs);

            if telegraf {
                let output =
                    Self::format_telegraf_output(&gpu_summaries, partitions);
                println!("{}", output);
                return;
            }

            let report = Self::format_cluster_report(
                &gpu_summaries,
                &cpu_summary,
                &queue_summary,
                !use_rich,
            );
            println!("{}", report);
        }
    }

    /// Format GPU usage data in InfluxDB Line Protocol for Telegraf.
    pub fn format_telegraf_output(
        summaries: &[GPUTypeSummary],
        partitions: Option<&[String]>,
    ) -> String {
        let mut lines = Vec::new();
        let timestamp_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        for summary in summaries {
            let mut tags = vec![format!("gpu_type={}", summary.gpu_type)];
            if let Some(parts) = partitions {
                let partition_str = parts
                    .join(",")
                    .replace(' ', "\\ ")
                    .replace(',', "\\,");
                tags.push(format!("partition={}", partition_str));
            }

            let fields = vec![
                format!("total={}i", summary.total_gpus),
                format!("used={}i", summary.used_gpus),
                format!("available={}i", summary.available_gpus),
                format!("utilization={:.2}", summary.utilization_percent()),
                format!("node_count={}i", summary.nodes_with_type.len()),
            ];

            lines.push(format!(
                "slurm_gpu_usage,{} {} {}",
                tags.join(","),
                fields.join(","),
                timestamp_ns
            ));
        }

        // Summary line for all GPUs
        let total_all: i32 = summaries.iter().map(|s| s.total_gpus).sum();
        let used_all: i32 = summaries.iter().map(|s| s.used_gpus).sum();
        let utilization_all = if total_all > 0 {
            used_all as f64 / total_all as f64 * 100.0
        } else {
            0.0
        };
        let total_nodes: usize = summaries
            .iter()
            .flat_map(|s| s.nodes_with_type.iter())
            .collect::<HashSet<_>>()
            .len();

        let mut tags = vec!["gpu_type=ALL".to_string()];
        if let Some(parts) = partitions {
            let partition_str = parts
                .join(",")
                .replace(' ', "\\ ")
                .replace(',', "\\,");
            tags.push(format!("partition={}", partition_str));
        }

        let fields = vec![
            format!("total={}i", total_all),
            format!("used={}i", used_all),
            format!("available={}i", total_all - used_all),
            format!("utilization={:.2}", utilization_all),
            format!("node_count={}i", total_nodes),
        ];

        lines.push(format!(
            "slurm_gpu_usage,{} {} {}",
            tags.join(","),
            fields.join(","),
            timestamp_ns
        ));

        lines.join("\n")
    }

}
