# slurm-gpu

GPU utilization reporting for Slurm clusters — a fast, single-binary Rust implementation.

Queries `sacct`, `scontrol`, and `squeue` to produce per-job efficiency reports, cluster-wide GPU usage summaries, and real-time job monitoring via `sstat`. Outputs can be rich terminal tables, plain text, or InfluxDB Line Protocol for Telegraf/Prometheus ingestion.

## Requirements

- Slurm workload manager with `sacct`, `squeue`, and `scontrol` commands
- Rust 1.70+ (build only)

## Building

```bash
cargo build --release
# Binary at target/release/slurm-gpu
```

## Installation

Copy the binary and create symlink shortcuts:

```bash
install -m 755 target/release/slurm-gpu /usr/local/bin/
ln -sf slurm-gpu /usr/local/bin/slurm-report
ln -sf slurm-gpu /usr/local/bin/slurm-usage
ln -sf slurm-gpu /usr/local/bin/slurm-stat
ln -sf slurm-gpu /usr/local/bin/slurm-show-tres
```

Symlinks invoke the corresponding subcommand automatically — `slurm-report` behaves exactly like `slurm-gpu report`.

## Commands

| Command | Symlink | Description |
|---------|---------|-------------|
| `slurm-gpu report` | `slurm-report` | GPU utilization reports from sacct job data |
| `slurm-gpu usage` | `slurm-usage` | Current GPU availability by type across the cluster |
| `slurm-gpu stat` | `slurm-stat` | Real-time monitoring of running jobs via sstat |
| `slurm-gpu show-tres` | `slurm-show-tres` | Display dynamic TRES ID mappings |

## Usage

### report — Job Efficiency Reports

```bash
# All users, last 24 hours (default)
slurm-report

# Specific user with time-weighted average row
slurm-report -u username -S yesterday

# Specific time range
slurm-report -S 2026-01-01 -E 2026-01-31

# GPU-only jobs, sorted by efficiency
slurm-report -g --sort-by gpu_eff --reverse

# Filter by partition and account
slurm-report -r compsci-gpu -A rescomp -a

# Find idle GPUs (util < 50% or mem_eff < 50%)
slurm-report --gpu-idle -S yesterday

# Summary report grouped by user
slurm-report --summary -S yesterday

# Summary grouped by user and partition
slurm-report --summary-by-partition -a -S yesterday

# Summary grouped by user and account
slurm-report --summary-by-account -a -S yesterday

# Include node, GPU type, and account columns
slurm-report --detailed -u username -S yesterday

# Save to file (plain text)
slurm-report --plain -o report.txt -u username -S yesterday

# Limit output
slurm-report --max-jobs 20 --min-gpu-eff 50
```

#### Telegraf Output (InfluxDB Line Protocol)

Output time-weighted average efficiency metrics in InfluxDB Line Protocol format, suitable for ingestion by Telegraf into Prometheus, InfluxDB, or Grafana.

```bash
# Single user
slurm-report --telegraf -u ukh -S 2026-02-03 -E 2026-02-04

# All users — one line per user
slurm-report --telegraf -a -S 2026-02-03 -E 2026-02-04

# Include partition tag
slurm-report --telegraf -u ukh -r h200alloc -S 2026-02-03

# Include account tag
slurm-report --telegraf -u ukh -A rescomp -S 2026-02-03
```

Example output:

```
slurm_gpu_efficiency,user=ukh,partition=h200alloc cpu_eff=1.3,mem_eff=59.9,gpu_eff=100.0,gpu_util=100.0,gpu_mem_eff=89.7,gpu_mem=125.6 1738800000000000000
```

**Measurement:** `slurm_gpu_efficiency`

| Tag | Description |
|-----|-------------|
| `user` | Slurm username (always present) |
| `partition` | Partition name (when `-r` is specified) |
| `account` | Slurm account (when `-A` is specified) |

| Field | Description |
|-------|-------------|
| `cpu_eff` | Time-weighted CPU efficiency (%) |
| `mem_eff` | Time-weighted memory efficiency (%) |
| `gpu_eff` | Time-weighted GPU efficiency (%) |
| `gpu_util` | Time-weighted GPU utilization (%) |
| `gpu_mem_eff` | Time-weighted GPU memory efficiency (%) |
| `gpu_mem` | GPU memory usage (GB) |

Fields with no data default to `0.0` so that every line has a consistent field set (required by Prometheus). The `--telegraf` flag is ignored when combined with `--summary*` flags.

#### Time-Weighted Average

When filtering by a specific user (`-u`), a **WEIGHTED AVERAGE** row appears at the bottom of the report. Each job's metric is weighted by its elapsed time:

```
weighted_avg = sum(elapsed_time * value) / sum(elapsed_time)
```

Longer-running jobs have proportionally more influence on the average.

### usage — Current GPU Availability

```bash
# Cluster-wide GPU availability
slurm-usage

# Specific user's GPU allocations
slurm-usage -u username

# All users breakdown
slurm-usage -a

# Filter by partition
slurm-usage -r compsci-gpu

# Common partition shortcut (-k = h200ea,education,gpu-common,scavenger-gpu)
slurm-usage -k

# Detailed view with node names
slurm-usage --detailed

# Telegraf output (InfluxDB line protocol)
slurm-usage --telegraf
```

### stat — Real-Time Job Monitoring

```bash
# Your running jobs (auto-filtered to current user)
slurm-stat

# Include node and GPU type details
slurm-stat --detailed

# Specific user
slurm-stat -u username

# Filter by partition
slurm-stat -r compsci-gpu

# Specific job IDs
slurm-stat -j 1234,5678

# Sort and limit
slurm-stat --sort-by gpu_eff --reverse --max-jobs 10
```

### show-tres — TRES ID Mappings

```bash
slurm-show-tres
```

Displays the dynamic TRES (Trackable Resource) ID-to-name mappings configured on the cluster.

## Report Columns

### Job Reports

| Column | Description |
|--------|-------------|
| User | Job owner |
| JobID | Slurm job identifier |
| State | Job state (COMPLETED, FAILED, RUNNING, etc.) |
| Elapsed | Job runtime (HH:MM:SS) |
| TimeEff | Time efficiency (elapsed / time limit) |
| CPUEff | CPU efficiency (CPU time used / allocated) |
| MemEff | Memory efficiency (peak memory / allocated) |
| GPUEff | GPU efficiency (normalized per-GPU average) |
| GPUUtil | Peak GPU utilization (total across all GPUs) |
| GPUMemEff | GPU memory efficiency (used / total capacity) |
| GPUMem | Peak GPU memory usage |
| Partition | Slurm partition name |

### Usage Reports

| Column | Description |
|--------|-------------|
| GPU Type | GPU model (a5000, h200, etc.) |
| Total | Total GPUs of this type |
| Used | Currently allocated GPUs |
| Available | GPUs available for new jobs |
| Utilization | Percentage of GPUs in use |
| Nodes | Number of nodes with this GPU type |

## Global Options

Most subcommands support these flags:

| Flag | Description |
|------|-------------|
| `--plain` | Plain text output (no colors or box-drawing) |
| `--detailed` | Include node, GPU type, and account columns |
| `--debug` | Verbose debug output to stderr |
| `-h`, `--help` | Print help |

## Architecture

```
src/
├── main.rs              # CLI entry point, subcommand dispatch, symlink detection
├── lib.rs               # Public module declarations
├── cli_helpers.rs       # Report option structs, job fetching, filtering, output routing
├── reporter.rs          # Table formatting (rich + plain + telegraf)
├── calculator.rs        # Efficiency calculations, time-weighted averages
├── models.rs            # Data models (GPUMetrics, SummaryMetrics, SlurmJob, etc.)
├── parser.rs            # sacct JSON output parsing
├── validation.rs        # Input validation and security
├── constants.rs         # Configuration constants and thresholds
├── errors.rs            # Custom error types
├── slurm_utils.rs       # Shared Slurm command utilities
├── gpu_usage.rs         # GPU usage reporting facade
├── gres_parser.rs       # GRES string parsing
├── cluster_data.rs      # Cluster data collection from scontrol
├── resource_slots.rs    # GPU/CPU slot collection
├── resource_summary.rs  # Usage summary generation
├── user_usage.rs        # Per-user resource usage tracking
├── queued_jobs.rs       # Pending job queue analysis
├── sstat.rs             # Real-time job monitoring via sstat
├── tres_parser.rs       # TRES string parsing
└── tres_registry.rs     # Dynamic TRES ID registry
```

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Check for warnings
cargo check

# Run directly
cargo run -- report -u username -S yesterday
cargo run -- usage --detailed
```

## License

MIT
