use comfy_table::{Cell, CellAlignment, Color};

use crate::calculator::{gpu_memory_mb, EfficiencyCalculator};
use crate::models::{JobInfoData, NodeTresUsage};
use crate::parser::SlurmJobParser;
use crate::scontrol_parser::parse_datetime_epoch;
use crate::slurm_utils::run_sacct_job_info;
use crate::table_helpers::{efficiency_cell, hdr, new_table, state_color};
use crate::validation::InputValidator;

pub struct JobInfoArgs {
    pub job_id: String,
    pub plain: bool,
    pub debug: bool,
}

pub fn run_job_info(args: JobInfoArgs) -> anyhow::Result<()> {
    InputValidator::validate_job_ids(&args.job_id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let raw = run_sacct_job_info(&args.job_id, args.debug)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let raw_fields = match extract_raw_job_fields(&raw) {
        Some(f) => f,
        None => {
            eprintln!("Error: Job {} not found.", args.job_id);
            eprintln!(
                "  Hint: Verify with: sacct -j {} --format=JobID,State",
                args.job_id
            );
            return Ok(());
        }
    };

    let jobs = match SlurmJobParser::parse_string(&raw) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Error parsing sacct output: {}", e);
            return Ok(());
        }
    };

    let job = match jobs.into_iter().find(|j| {
        j.job_id.to_string() == args.job_id
            || j.job_id.to_string().starts_with(&args.job_id)
    }) {
        Some(j) => j,
        None => {
            eprintln!("Error: Job {} not found.", args.job_id);
            eprintln!(
                "  Hint: Verify with: sacct -j {} --format=JobID,State",
                args.job_id
            );
            return Ok(());
        }
    };

    let partition_limits = crate::slurm_utils::get_partition_time_limits(args.debug);
    let metric = EfficiencyCalculator::calculate_metrics(&job, partition_limits, args.debug);

    let allocated = EfficiencyCalculator::get_allocated_resources(&job);
    let (_, cpu_peak_eff_f64, _) = EfficiencyCalculator::get_cpu_and_mem_metrics(&job);
    let cpu_peak_eff = format_cpu_peak_eff(cpu_peak_eff_f64);
    let gpu_type_str = EfficiencyCalculator::detect_gpu_type(&job);
    let gpu_type: Option<String> = if gpu_type_str == "default" {
        None
    } else {
        Some(gpu_type_str.clone())
    };
    let gpu_total_mem_mb: Option<f64> = gpu_type.as_deref().map(|t| {
        let mb = gpu_memory_mb(t);
        if mb == 24576 && t != "a5000" {
            // default fallback — treat as unknown
            return -1.0;
        }
        mb as f64
    }).filter(|&v| v > 0.0);

    // Parse per-node TRES data (empty when TRES accounting is not enabled)
    let max_nodes = SlurmJobParser::parse_per_node_tres(&raw_fields.tres_in_max_str);
    let tot_nodes = SlurmJobParser::parse_per_node_tres(&raw_fields.tres_in_tot_str);
    let node_usage = merge_node_tres(max_nodes, tot_nodes, allocated.cpu, allocated.mem as f64, job.allocation_nodes.unwrap_or(1));

    let state_str = job.state.current.first().cloned().unwrap_or_default();

    let data = JobInfoData {
        job_id: args.job_id.clone(),
        user: job.association.user.clone(),
        account: job.association.account.clone(),
        job_name: job.name.clone(),
        state: state_str,
        num_nodes: job.allocation_nodes.unwrap_or(1),
        num_cpus: allocated.cpu,
        mem_alloc_mb: allocated.mem as f64,
        num_gpus: allocated.gpu,
        gpu_type,
        gpu_total_mem_mb,
        qos: raw_fields.qos.clone(),
        partition: job.association.partition.clone(),
        cluster: raw_fields.cluster.clone(),
        start_epoch: parse_datetime_epoch(&raw_fields.start_str),
        elapsed_seconds: job.time.elapsed,
        time_limit_seconds: job.time.limit.as_ref().and_then(|lim| {
            if lim.set && !lim.infinite {
                Some(lim.number * 60)
            } else {
                None
            }
        }),
        cpu_eff: metric.cpu_eff.clone(),
        cpu_peak_eff,
        mem_eff: metric.mem_eff.clone(),
        gpu_eff: metric.gpu_eff.clone(),
        gpu_util: metric.gpu_util.clone(),
        gpu_mem_eff: metric.gpu_mem_eff.clone(),
        time_eff: metric.time_eff.clone(),
        node_usage,
    };

    print_job_info(&data, args.plain);
    Ok(())
}

// ---------------------------------------------------------------------------
// Raw field extraction
// ---------------------------------------------------------------------------

struct RawJobFields {
    qos: String,
    cluster: String,
    start_str: String,
    tres_in_max_str: String,
    tres_in_tot_str: String,
}

fn extract_raw_job_fields(raw: &str) -> Option<RawJobFields> {
    let mut lines = raw.lines();

    // First non-empty line is the header
    let header_line = lines.find(|l| !l.trim().is_empty())?;
    let headers: Vec<&str> = header_line.split('\t').collect();

    let col = |name: &str| -> Option<usize> {
        headers.iter().position(|h| h.trim().eq_ignore_ascii_case(name))
    };

    let idx_jobid = col("JobID")?;
    let idx_qos = col("QOS");
    let idx_cluster = col("Cluster");
    let idx_start = col("Start");
    let idx_tres_max = col("TresUsageInMax");
    let idx_tres_tot = col("TresUsageInTot");

    // Find the base job row (JobID has no '.' — not a step)
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        let job_id_val = cols.get(idx_jobid).map(|s| s.trim()).unwrap_or("");
        if job_id_val.contains('.') || job_id_val.is_empty() {
            continue;
        }

        let get = |idx: Option<usize>| -> String {
            idx.and_then(|i| cols.get(i)).map(|s| s.trim().to_string()).unwrap_or_default()
        };

        return Some(RawJobFields {
            qos: get(idx_qos),
            cluster: get(idx_cluster),
            start_str: get(idx_start),
            tres_in_max_str: get(idx_tres_max),
            tres_in_tot_str: get(idx_tres_tot),
        });
    }

    None
}

// ---------------------------------------------------------------------------
// Per-node merge
// ---------------------------------------------------------------------------

fn merge_node_tres(
    max_records: Vec<NodeTresUsage>,
    tot_records: Vec<NodeTresUsage>,
    total_cpus: i32,
    total_mem_mb: f64,
    num_nodes: i32,
) -> Vec<NodeTresUsage> {
    if max_records.is_empty() && tot_records.is_empty() {
        return Vec::new();
    }

    let num_nodes = num_nodes.max(1);
    let cpu_per_node = total_cpus / num_nodes;
    let mem_per_node = total_mem_mb / num_nodes as f64;

    // Build map from tot_records keyed by node_name
    let mut tot_map: std::collections::HashMap<String, NodeTresUsage> = tot_records
        .into_iter()
        .map(|r| (r.node_name.clone(), r))
        .collect();

    let mut results: Vec<NodeTresUsage> = max_records
        .into_iter()
        .map(|mut max_rec| {
            if let Some(tot_rec) = tot_map.remove(&max_rec.node_name) {
                max_rec.cpu_seconds_used = tot_rec.cpu_seconds_used;
            }
            max_rec.cpu_alloc_per_node = cpu_per_node;
            max_rec.mem_alloc_mb = mem_per_node;
            max_rec
        })
        .collect();

    // Add any tot_records not in max_records
    for (_, mut rec) in tot_map {
        rec.cpu_alloc_per_node = cpu_per_node;
        rec.mem_alloc_mb = mem_per_node;
        results.push(rec);
    }

    results
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub fn print_job_info(data: &JobInfoData, _plain: bool) {
    print_metadata_table(data);
    println!();
    print_overall_efficiency(data);

    if !data.node_usage.is_empty() {
        println!();
        print_node_breakdown(data);

        let has_gpu_devices = data.node_usage.iter().any(|n| !n.gpu_util_by_index.is_empty());
        if has_gpu_devices {
            println!();
            print_gpu_breakdown(data);
        }
    }

    let notes = advisory_notes(data);
    if !notes.is_empty() {
        println!();
        println!("Notes:");
        for note in &notes {
            println!("  * {}", note);
        }
    }
}

fn print_metadata_table(data: &JobInfoData) {
    let mut table = new_table();
    table.set_header(vec![
        hdr("Field"),
        hdr("Value"),
    ]);

    let gpu_str = if data.num_gpus > 0 {
        match &data.gpu_type {
            Some(t) => format!("{} × {}", data.num_gpus, t),
            None => data.num_gpus.to_string(),
        }
    } else {
        "---".to_string()
    };

    let mem_gb = data.mem_alloc_mb / 1024.0;
    let mem_per_cpu = if data.num_cpus > 0 {
        format!(" ({:.1} GB/core)", mem_gb / data.num_cpus as f64)
    } else {
        String::new()
    };
    let mem_str = format!("{:.0} GB{}", mem_gb, mem_per_cpu);

    let qos_partition = if data.qos.is_empty() {
        data.partition.clone()
    } else {
        format!("{}/{}", data.qos, data.partition)
    };

    let start_str = data
        .start_epoch
        .map(format_start_time)
        .unwrap_or_else(|| "---".to_string());

    let elapsed_str = format_duration(data.elapsed_seconds);
    let limit_str = data
        .time_limit_seconds
        .map(format_duration)
        .unwrap_or_else(|| "---".to_string());

    let rows: Vec<(&str, Cell)> = vec![
        ("Job ID",       Cell::new(&data.job_id).fg(Color::Cyan)),
        ("User/Account", Cell::new(format!("{}/{}", data.user, data.account)).fg(Color::Magenta)),
        ("Job Name",     Cell::new(&data.job_name)),
        ("State",        Cell::new(&data.state).fg(state_color(&data.state))),
        ("Nodes",        Cell::new(data.num_nodes.to_string())),
        ("CPU Cores",    Cell::new(data.num_cpus.to_string())),
        ("CPU Memory",   Cell::new(&mem_str)),
        ("GPUs",         Cell::new(&gpu_str)),
        ("QOS/Partition",Cell::new(&qos_partition)),
        ("Cluster",      Cell::new(if data.cluster.is_empty() { "---" } else { &data.cluster })),
        ("Start Time",   Cell::new(&start_str)),
        ("Run Time",     Cell::new(&elapsed_str)),
        ("Time Limit",   Cell::new(&limit_str)),
    ];

    for (label, value_cell) in rows {
        table.add_row(vec![
            Cell::new(label).fg(Color::White).set_alignment(CellAlignment::Right),
            value_cell,
        ]);
    }

    println!("{table}");
}

fn print_overall_efficiency(data: &JobInfoData) {
    let mut table = new_table();
    table.set_header(vec![
        hdr("Metric"),
        hdr("Efficiency"),
    ]);

    let rows = [
        ("CPU Efficiency",  &data.cpu_eff),
        ("CPU Peak Core",   &data.cpu_peak_eff),
        ("CPU Memory",      &data.mem_eff),
        ("GPU Efficiency",  &data.gpu_eff),
        ("GPU Utilization", &data.gpu_util),
        ("GPU Mem Eff",     &data.gpu_mem_eff),
        ("Time Efficiency", &data.time_eff),
    ];

    for (label, value) in &rows {
        if label.contains("GPU") && data.num_gpus == 0 {
            continue;
        }
        table.add_row(vec![
            Cell::new(*label).fg(Color::White),
            efficiency_cell(value).set_alignment(CellAlignment::Right),
        ]);
    }

    println!("{table}");
}

fn print_node_breakdown(data: &JobInfoData) {
    let mut table = new_table();
    table.set_header(vec![
        hdr("Node"),
        hdr("CPU Time Used / Total"),
        hdr("Mem Used / Alloc"),
        hdr("CPU Eff"),
    ]);

    let elapsed = data.elapsed_seconds;

    for node in &data.node_usage {
        let total_cpu_secs = node.cpu_alloc_per_node as i64 * elapsed;
        let cpu_used_str = format_duration(node.cpu_seconds_used);
        let cpu_total_str = format_duration(total_cpu_secs);
        let cpu_time_str = format!("{}/{}", cpu_used_str, cpu_total_str);

        let cpu_eff_pct = if total_cpu_secs > 0 {
            node.cpu_seconds_used as f64 / total_cpu_secs as f64 * 100.0
        } else {
            0.0
        };
        let cpu_eff_str = format!("{:.1}%", cpu_eff_pct);

        let mem_used_gb = node.mem_used_mb / 1024.0;
        let mem_alloc_gb = node.mem_alloc_mb / 1024.0;
        let mem_str = format!("{:.1}/{:.1} GB", mem_used_gb, mem_alloc_gb);

        table.add_row(vec![
            Cell::new(&node.node_name).fg(Color::Green),
            Cell::new(&cpu_time_str),
            Cell::new(&mem_str),
            efficiency_cell(&cpu_eff_str).set_alignment(CellAlignment::Right),
        ]);
    }

    println!("{table}");
}

fn print_gpu_breakdown(data: &JobInfoData) {
    let mut table = new_table();
    table.set_header(vec![
        hdr("Node"),
        hdr("GPU"),
        hdr("GPU Util"),
        hdr("GPU Mem Used / Total"),
    ]);

    for node in &data.node_usage {
        // Collect all device indices present in either util or mem
        let mut indices: Vec<u32> = node
            .gpu_util_by_index
            .iter()
            .map(|(i, _)| *i)
            .chain(node.gpu_mem_used_by_index.iter().map(|(i, _)| *i))
            .collect();
        indices.sort_unstable();
        indices.dedup();

        for idx in indices {
            let util_str = node
                .gpu_util_by_index
                .iter()
                .find(|(i, _)| *i == idx)
                .map(|(_, pct)| format!("{:.1}%", pct))
                .unwrap_or_else(|| "---".to_string());

            let mem_str = node
                .gpu_mem_used_by_index
                .iter()
                .find(|(i, _)| *i == idx)
                .map(|(_, used_mb)| {
                    let used_gb = used_mb / 1024.0;
                    match data.gpu_total_mem_mb {
                        Some(total_mb) => format!("{:.1}/{:.1} GB", used_gb, total_mb / 1024.0),
                        None => format!("{:.1} GB", used_gb),
                    }
                })
                .unwrap_or_else(|| "---".to_string());

            table.add_row(vec![
                Cell::new(&node.node_name).fg(Color::Green),
                Cell::new(format!("GPU {}", idx)).fg(Color::Yellow),
                efficiency_cell(&util_str).set_alignment(CellAlignment::Right),
                Cell::new(&mem_str),
            ]);
        }
    }

    println!("{table}");
}

// ---------------------------------------------------------------------------
// Advisory notes
// ---------------------------------------------------------------------------

fn advisory_notes(data: &JobInfoData) -> Vec<String> {
    let mut notes = Vec::new();

    let parse_pct = |s: &str| -> Option<f64> {
        s.trim_end_matches('%').parse::<f64>().ok()
    };

    if let Some(pct) = parse_pct(&data.mem_eff) {
        if pct < 20.0 && data.mem_alloc_mb > 0.0 {
            notes.push(format!(
                "This job only used {:.0}% of {} GB of allocated CPU memory. \
                Consider reducing memory with --mem-per-cpu or --mem.",
                pct,
                data.mem_alloc_mb / 1024.0
            ));
        }
    }

    if let Some(pct) = parse_pct(&data.cpu_eff) {
        if pct < 25.0 && data.num_cpus > 1 {
            notes.push(format!(
                "CPU efficiency is {:.0}%. Consider reducing --cpus-per-task or \
                reviewing thread/parallelism settings.",
                pct
            ));
        }
    }

    if data.num_gpus > 0 {
        if let Some(pct) = parse_pct(&data.gpu_util) {
            if pct < 30.0 {
                notes.push(format!(
                    "GPU utilization is {:.0}%. Consider profiling the GPU workload \
                    or reducing the number of GPUs requested.",
                    pct
                ));
            }
        }
    }

    if data.state == "COMPLETED" {
        if let (Some(elapsed), Some(limit)) =
            (Some(data.elapsed_seconds), data.time_limit_seconds)
        {
            if limit > 0 && (elapsed as f64 / limit as f64) < 0.10 {
                notes.push(format!(
                    "Job ran for {} but the time limit was {}. \
                    Consider reducing --time to improve queue priority.",
                    format_duration(elapsed),
                    format_duration(limit)
                ));
            }
        }
    }

    notes
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn format_cpu_peak_eff(value: f64) -> String {
    if value < 0.0 {
        "---".to_string()
    } else if value >= 0.05 {
        format!("{:.1}%", value)
    } else if value > 0.0 {
        "<0.1%".to_string()
    } else {
        "---".to_string()
    }
}

fn format_start_time(epoch: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(epoch, 0).single() {
        Some(dt) => dt.format("%a %b %-d, %Y at %-I:%M %p").to_string(),
        None => "---".to_string(),
    }
}

pub(crate) fn format_duration(secs: i64) -> String {
    if secs <= 0 {
        return "---".to_string();
    }
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if days > 0 {
        format!("{}-{:02}:{:02}:{:02}", days, hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::SlurmJobParser;

    #[test]
    fn format_duration_sub_day() {
        assert_eq!(format_duration(67316), "18:41:56");
    }

    #[test]
    fn format_duration_multi_day() {
        assert_eq!(format_duration(86400 + 3661), "1-01:01:01");
    }

    #[test]
    fn format_duration_zero() {
        assert_eq!(format_duration(0), "---");
    }

    #[test]
    fn advisory_notes_low_mem() {
        let data = JobInfoData {
            state: "COMPLETED".to_string(),
            mem_eff: "6.0%".to_string(),
            mem_alloc_mb: 256.0 * 1024.0,
            cpu_eff: "50.0%".to_string(),
            gpu_util: "---".to_string(),
            time_eff: "50.0%".to_string(),
            time_limit_seconds: Some(3600),
            elapsed_seconds: 1800,
            ..Default::default()
        };
        let notes = advisory_notes(&data);
        assert!(notes.iter().any(|n| n.contains("memory")), "expected memory note, got: {:?}", notes);
    }

    #[test]
    fn advisory_notes_low_time_eff() {
        let data = JobInfoData {
            state: "COMPLETED".to_string(),
            mem_eff: "80.0%".to_string(),
            cpu_eff: "80.0%".to_string(),
            gpu_util: "---".to_string(),
            time_eff: "5.0%".to_string(),
            time_limit_seconds: Some(86400),
            elapsed_seconds: 1000,
            ..Default::default()
        };
        let notes = advisory_notes(&data);
        assert!(notes.iter().any(|n| n.contains("time limit")), "expected time note, got: {:?}", notes);
    }

    #[test]
    fn parse_per_node_tres_empty() {
        let result = SlurmJobParser::parse_per_node_tres("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_per_node_tres_single_node() {
        let tres = "cpu=01:00:00,mem=8192M,gres/gpuutil:0=65.7,gres/gpuutil:1=72.3,node=node01";
        let result = SlurmJobParser::parse_per_node_tres(tres);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].node_name, "node01");
        assert_eq!(result[0].cpu_seconds_used, 3600);
        assert_eq!(result[0].mem_used_mb, 8192.0);
        assert_eq!(result[0].gpu_util_by_index.len(), 2);
        assert_eq!(result[0].gpu_util_by_index[0], (0, 65.7));
        assert_eq!(result[0].gpu_util_by_index[1], (1, 72.3));
    }

    #[test]
    fn parse_per_node_tres_two_nodes() {
        let tres = "cpu=01:00:00,mem=4096M,gres/gpuutil:0=65.7,node=node01|cpu=00:30:00,mem=4096M,gres/gpuutil:0=72.9,node=node02";
        let result = SlurmJobParser::parse_per_node_tres(tres);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].node_name, "node01");
        assert_eq!(result[1].node_name, "node02");
    }

    #[test]
    fn parse_per_node_tres_no_node_qualifier_ignored() {
        // A flat TRES string with no node= should yield an empty vec
        let tres = "cpu=01:00:00,mem=8192M,gres/gpuutil=65.7";
        let result = SlurmJobParser::parse_per_node_tres(tres);
        assert!(result.is_empty());
    }

    #[test]
    fn print_job_info_no_node_usage() {
        // Smoke test: should not panic and should produce output
        let data = JobInfoData {
            job_id: "12345".to_string(),
            user: "testuser".to_string(),
            account: "testacct".to_string(),
            job_name: "test_job".to_string(),
            state: "COMPLETED".to_string(),
            num_nodes: 1,
            num_cpus: 4,
            mem_alloc_mb: 16384.0,
            num_gpus: 1,
            gpu_type: Some("a100".to_string()),
            gpu_total_mem_mb: Some(81920.0),
            qos: "gpu".to_string(),
            partition: "test".to_string(),
            cluster: "testcluster".to_string(),
            start_epoch: Some(1646373360),
            elapsed_seconds: 3600,
            time_limit_seconds: Some(14400),
            cpu_eff: "75.0%".to_string(),
            cpu_peak_eff: "12.5%".to_string(),
            mem_eff: "50.0%".to_string(),
            gpu_eff: "80.0%".to_string(),
            gpu_util: "78.0%".to_string(),
            gpu_mem_eff: "65.0%".to_string(),
            time_eff: "25.0%".to_string(),
            node_usage: vec![],
        };
        // Capture that print_job_info doesn't panic
        print_job_info(&data, false);
    }
}
