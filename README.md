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

### From git (recommended)

```bash
cargo install --git git@gitlab.oit.duke.edu:wjs/rusty-slurmgpu.git
```

This installs the `slurm-gpu` binary to `~/.cargo/bin/`. Then run the install script to create symlinks, shell completions, and man pages:

```bash
git clone git@gitlab.oit.duke.edu:wjs/rusty-slurmgpu.git
cd rusty-slurmgpu
./install.sh              # installs extras to ~/.local (default)
./install.sh /usr/local   # or specify a custom prefix
```

### From release tarball

Download from the [Releases page](https://gitlab.oit.duke.edu/wjs/rusty-slurmgpu/-/releases):

```bash
tar xzf slurm-gpu-0.1.0-linux-x86_64.tar.gz
cd slurm-gpu-0.1.0-linux-x86_64
./install.sh
```

### From source

```bash
git clone git@gitlab.oit.duke.edu:wjs/rusty-slurmgpu.git
cd rusty-slurmgpu
cargo build --release
./install.sh
```

Symlinks invoke the corresponding subcommand automatically — `slurm-report` behaves exactly like `slurm-gpu report`.

## Commands

| Command | Symlink | Description |
|---------|---------|-------------|
| `slurm-gpu report` | `slurm-report` | GPU utilization reports from sacct job data |
| `slurm-gpu usage` | `slurm-usage` | Current GPU availability by type across the cluster |
| `slurm-gpu stat` | `slurm-stat` | Real-time monitoring of running jobs via sstat |
| `slurm-gpu show-tres` | `slurm-show-tres` | Display dynamic TRES ID mappings |
| `slurm-gpu tui` | `slurm-tui` | Interactive terminal dashboard |
| `slurm-gpu completions` | — | Generate shell completion scripts |

Running `slurm-gpu` with no subcommand defaults to `usage`.

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

### tui — Interactive Dashboard

```bash
# Launch with defaults (auto-filtered to current user)
slurm-tui

# Specific user and partition
slurm-tui -u username -r compsci-gpu

# Custom start time and refresh interval
slurm-tui -S yesterday --refresh 60
```

Three-tab dashboard built with ratatui:

| Tab | Content |
|-----|---------|
| Report | sacct job history with efficiency metrics |
| Usage | Cluster-wide GPU/CPU overview |
| My Jobs | Live sstat monitoring of running jobs |

**Key bindings:**

| Key | Action |
|-----|--------|
| `q` | Quit |
| `1` / `2` / `3` | Switch tab (Report / Usage / My Jobs) |
| `Tab` / `Shift+Tab` | Next / previous tab |
| `r` | Manual refresh |
| `p` | Pause / resume auto-refresh |
| `s` | Cycle state filter |
| `+` / `-` | Increase / decrease refresh interval (5s steps) |
| `/` | Focus search input |
| `Esc` | Clear search / unfocus |
| `?` | Toggle help popup |
| `Up` / `Down` | Navigate table rows |
| `Home` / `End` | Jump to first / last row |

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
├── table_helpers.rs     # Shared table construction utilities (comfy-table)
├── calculator.rs        # Efficiency calculations, time-weighted averages
├── models.rs            # Data models (GPUMetrics, SummaryMetrics, SlurmJob, etc.)
├── parser.rs            # sacct JSON output parsing
├── validation.rs        # Input validation and security
├── constants.rs         # Configuration constants and thresholds
├── errors.rs            # Custom error types
├── command_ext.rs       # Command timeout and execution helpers
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
├── tres_registry.rs     # Dynamic TRES ID registry
└── tui/
    ├── mod.rs           # TUI module root, TuiConfig
    ├── app.rs           # Application loop, terminal setup, rendering
    ├── data.rs          # Background data fetching threads
    ├── input.rs         # Text input field widget
    ├── widgets.rs       # Shared TUI widgets (efficiency colors, help popup)
    ├── report_tab.rs    # Report tab rendering
    ├── usage_tab.rs     # Usage tab rendering
    └── stat_tab.rs      # My Jobs (sstat) tab rendering
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
cargo run -- tui -S yesterday
```

## Shell Completions

Generate and install completion scripts for your shell:

```bash
# Bash
slurm-gpu completions bash > /etc/bash_completion.d/slurm-gpu

# Zsh
slurm-gpu completions zsh > "${fpath[1]}/_slurm-gpu"

# Fish
slurm-gpu completions fish > ~/.config/fish/completions/slurm-gpu.fish
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `SLURM_GPU_K_PARTITIONS` | Override the `-k` partition shortcut (comma-separated list) |

## License

MIT
