use anyhow::Result;
use clap::{Parser, Subcommand};
use std::env;

use slurm_gpu_reporter::cli_helpers::*;
use slurm_gpu_reporter::gpu_usage::GPUUsageReporter;
use slurm_gpu_reporter::reporter::GPUReporter;
use slurm_gpu_reporter::slurm_utils::{build_node_gpu_mapping, sort_metrics};
use slurm_gpu_reporter::sstat::SstatMonitor;
use slurm_gpu_reporter::tres_registry::TresRegistry;

#[derive(Parser)]
#[command(
    name = "slurm-gpu",
    version,
    about = "GPU utilization reporting for Slurm",
    long_about = "GPU utilization reporting for Slurm clusters.\n\n\
        Queries sacct, scontrol, and squeue to produce per-job efficiency reports,\n\
        cluster-wide GPU usage summaries, and real-time job monitoring via sstat.\n\n\
        Symlink shortcuts: slurm-report, slurm-usage, slurm-stat, slurm-show-tres"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate GPU utilization reports by querying sacct directly
    Report(ReportArgs),

    /// Generate current GPU usage report by GPU type across the cluster
    Usage(UsageArgs),

    /// Monitor running jobs with real-time GPU and CPU statistics using sstat
    Stat(StatArgs),

    /// Show dynamic TRES ID mappings for this cluster
    ShowTres,
}

#[derive(Parser, Debug)]
pub struct ReportArgs {
    /// Start time for sacct query (e.g., 2025-07-24, yesterday, today)
    #[arg(short = 'S', long)]
    starttime: Option<String>,

    /// End time for sacct query (e.g., 2025-07-24, today, now)
    #[arg(short = 'E', long)]
    endtime: Option<String>,

    /// Slurm partition to query
    #[arg(short = 'r', long = "partition")]
    partition_filter: Option<String>,

    /// Slurm account to query (comma-separated list)
    #[arg(short = 'A', long = "account")]
    account_filter: Option<String>,

    /// Specific user to query
    #[arg(short = 'u', long)]
    user: Option<String>,

    /// Query all users (default when no user specified)
    #[arg(short = 'a', long)]
    allusers: bool,

    /// Comma-separated list of specific job IDs to query
    #[arg(short = 'j', long)]
    jobs: Option<String>,

    /// Only show jobs that request GPU resources
    #[arg(short = 'g', long)]
    gpu: bool,

    /// Show only jobs with GPU utilization < 50% OR GPU memory efficiency < 50%
    #[arg(long)]
    gpu_idle: bool,

    /// Generate summary report grouped by user
    #[arg(long)]
    summary: bool,

    /// Generate summary report grouped by user and partition
    #[arg(long)]
    summary_by_partition: bool,

    /// Generate summary report grouped by user and account
    #[arg(long)]
    summary_by_account: bool,

    /// Include node and GPU type columns
    #[arg(long)]
    detailed: bool,

    /// Use plain text output instead of rich formatting
    #[arg(long)]
    plain: bool,

    /// Enable debug output
    #[arg(long)]
    debug: bool,

    /// Output file (default: stdout)
    #[arg(short = 'o', long)]
    output: Option<String>,

    /// Filter jobs by state
    #[arg(long, default_value = "all", value_parser = ["all", "completed", "failed", "pending", "running"])]
    filter_state: String,

    /// Only show jobs with GPU efficiency >= this value
    #[arg(long)]
    min_gpu_eff: Option<f64>,

    /// Maximum number of jobs to display
    #[arg(long)]
    max_jobs: Option<usize>,

    /// Sort jobs by specified field
    #[arg(long, default_value = "job_id", value_parser = ["user", "job_id", "state", "elapsed", "gpu_eff", "gpu_mem_eff", "time_eff", "cpu_eff", "mem_eff"])]
    sort_by: String,

    /// Reverse sort order
    #[arg(long)]
    reverse: bool,

    /// Include partition information in the report
    #[arg(long, default_value_t = true)]
    show_partition: bool,

    /// Output weighted averages in InfluxDB line protocol format for Telegraf.
    /// Use with -u for a single user or -a for one line per user.
    /// Ignored with --summary* flags.
    #[arg(long)]
    telegraf: bool,
}

#[derive(Parser, Debug)]
pub struct UsageArgs {
    /// Use plain text output instead of rich formatting
    #[arg(long)]
    plain: bool,

    /// Show detailed report with node names
    #[arg(long)]
    detailed: bool,

    /// Show only nodes in the specified partition(s). Can be comma-separated list
    #[arg(short = 'r', long = "partition")]
    partition: Option<String>,

    /// Shortcut for partitions: h200ea,education,gpu-common,scavenger-gpu
    #[arg(short = 'k')]
    k: bool,

    /// Show GPU usage for a specific user
    #[arg(short = 'u', long)]
    user: Option<String>,

    /// Show GPU usage breakdown by all users
    #[arg(short = 'a', long = "all")]
    all_users: bool,

    /// Output in InfluxDB line protocol format for Telegraf
    #[arg(long)]
    telegraf: bool,

    /// Enable debug output
    #[arg(long)]
    debug: bool,
}

#[derive(Parser, Debug)]
pub struct StatArgs {
    /// Use plain text output instead of rich formatting
    #[arg(long)]
    plain: bool,

    /// Include node and GPU type columns
    #[arg(long)]
    detailed: bool,

    /// Specific user to query
    #[arg(short = 'u', long)]
    user: Option<String>,

    /// Slurm partition to query
    #[arg(short = 'r', long = "partition")]
    partition_filter: Option<String>,

    /// Comma-separated list of specific job IDs to query
    #[arg(short = 'j', long)]
    jobs: Option<String>,

    /// Maximum number of jobs to display (default: 50)
    #[arg(long, default_value_t = 50)]
    max_jobs: usize,

    /// Sort jobs by specified field
    #[arg(long, default_value = "job_id", value_parser = ["user", "job_id", "elapsed", "gpu_eff", "gpu_mem_eff", "time_eff", "cpu_eff", "mem_eff"])]
    sort_by: String,

    /// Reverse sort order
    #[arg(long)]
    reverse: bool,

    /// Enable debug output
    #[arg(long)]
    debug: bool,
}

fn main() -> Result<()> {
    // Check argv[0] for symlink dispatch
    let argv0 = env::args()
        .next()
        .unwrap_or_default();
    let binary_name = std::path::Path::new(&argv0)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("slurm-gpu");

    match binary_name {
        "slurm-report" => {
            let args = ReportArgs::parse();
            run_report(args);
            return Ok(());
        }
        "slurm-usage" => {
            let args = UsageArgs::parse();
            run_usage(args);
            return Ok(());
        }
        "slurm-stat" => {
            let args = StatArgs::parse();
            run_stat(args);
            return Ok(());
        }
        "slurm-show-tres" => {
            run_show_tres();
            return Ok(());
        }
        _ => {}
    }

    // Normal subcommand dispatch
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Report(args)) => run_report(args),
        Some(Commands::Usage(args)) => run_usage(args),
        Some(Commands::Stat(args)) => run_stat(args),
        Some(Commands::ShowTres) => run_show_tres(),
        None => {
            println!("slurm-gpu: GPU utilization reporting for Slurm");
            println!("Use --help for usage information");
            println!();
            println!("Available commands:");
            println!("  report      Generate GPU utilization reports");
            println!("  usage       Show current GPU usage by type");
            println!("  stat        Monitor running jobs with real-time stats");
            println!("  show-tres   Show dynamic TRES ID mappings");
            println!();
            println!("Symlink shortcuts:");
            println!("  slurm-report     -> slurm-gpu report");
            println!("  slurm-usage      -> slurm-gpu usage");
            println!("  slurm-stat       -> slurm-gpu stat");
            println!("  slurm-show-tres  -> slurm-gpu show-tres");
        }
    }

    Ok(())
}

fn run_report(args: ReportArgs) {
    let user_specified_time = args.starttime.is_some() || args.endtime.is_some();

    let options = ReportOptions {
        starttime: args.starttime,
        endtime: args.endtime,
        partition_filter: args.partition_filter,
        account_filter: args.account_filter,
        user: args.user,
        allusers: args.allusers,
        jobs: args.jobs,
        gpu: args.gpu,
        gpu_idle: args.gpu_idle,
        summary: args.summary,
        summary_by_partition: args.summary_by_partition,
        summary_by_account: args.summary_by_account,
        detailed: args.detailed,
        plain: args.plain,
        debug: args.debug,
        output: args.output,
        filter_state: args.filter_state,
        min_gpu_eff: args.min_gpu_eff,
        max_jobs: args.max_jobs,
        sort_by: args.sort_by,
        reverse: args.reverse,
        show_partition: args.show_partition,
        telegraf: args.telegraf,
        user_specified_time,
    };

    // Fetch and parse jobs
    let all_jobs = fetch_and_parse_jobs(&options);

    // Filter for GPU jobs if requested
    let all_jobs = if options.gpu {
        filter_gpu_jobs(all_jobs, options.debug)
    } else {
        all_jobs
    };

    if options.debug {
        eprintln!("Found {} jobs", all_jobs.len());
    }

    // Calculate metrics
    let metrics = calculate_metrics_for_jobs(&all_jobs, options.debug);

    // Apply filters
    let metrics = filter_metrics(metrics, &options);

    // Sort metrics
    let mut metrics = metrics;
    sort_metrics(&mut metrics, &options.sort_by, options.reverse);

    // Generate output
    if options.summary || options.summary_by_partition || options.summary_by_account {
        if options.telegraf {
            eprintln!("Warning: --telegraf is not supported with summary reports, ignoring --telegraf");
        }
        generate_and_output_summary(&metrics, &options);
    } else {
        generate_and_output_report(&metrics, &options);
    }
}

fn run_usage(args: UsageArgs) {
    let mut partition = args.partition;

    // Handle the -k shortcut
    if args.k {
        if partition.is_some() {
            eprintln!("Warning: -k flag overrides --partition option");
        }
        partition = Some("h200ea,education,gpu-common,scavenger-gpu".to_string());
    }

    let partitions: Option<Vec<String>> = partition.map(|p| {
        p.split(',').map(|s| s.trim().to_string()).collect()
    });
    let partitions_ref = partitions.as_deref();

    GPUUsageReporter::print_usage_report(
        !args.plain,
        args.detailed,
        args.debug,
        partitions_ref,
        args.user.as_deref(),
        args.all_users,
        args.telegraf,
    );
}

fn run_stat(args: StatArgs) {
    // Parse job IDs if provided
    let job_ids: Option<Vec<String>> = args.jobs.map(|j| {
        j.split(',').map(|s| s.trim().to_string()).collect()
    });

    // Auto-filter to current user unless root or specific user requested
    let current_user = users::get_current_username()
        .map(|u| u.to_string_lossy().to_string())
        .unwrap_or_default();

    let effective_user = if let Some(ref u) = args.user {
        if u != &current_user && current_user != "root" {
            eprintln!(
                "Note: Filtering to user '{}' but you can only see detailed statistics for your own jobs.",
                u
            );
        }
        Some(u.clone())
    } else if current_user != "root" {
        if args.debug {
            eprintln!("Debug: Auto-filtering to current user: {}", current_user);
        }
        Some(current_user)
    } else {
        None
    };

    // Build node-to-GPU mapping
    if args.debug {
        eprintln!("Debug: Building node-to-GPU mapping...");
    }
    let node_gpu_map = build_node_gpu_mapping(args.debug);

    // Monitor jobs
    let metrics = SstatMonitor::monitor_jobs(
        effective_user.as_deref(),
        args.partition_filter.as_deref(),
        job_ids.as_deref(),
        args.max_jobs,
        &node_gpu_map,
        args.debug,
    );

    if metrics.is_empty() {
        eprintln!("No running jobs found matching the criteria.");
        return;
    }

    // Sort metrics
    let mut metrics = metrics;
    sort_metrics(&mut metrics, &args.sort_by, args.reverse);

    // Display report
    GPUReporter::print_report(&metrics, !args.plain, true, args.detailed, false);
}

fn run_show_tres() {
    let registry = TresRegistry::get_instance();
    registry.debug_print_tres_map();
}
