use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Job ID that can be numeric or an array task string like "1234_56"
#[derive(Debug, Clone, Serialize)]
pub enum JobId {
    Numeric(i64),
    ArrayTask(String),
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobId::Numeric(n) => write!(f, "{}", n),
            JobId::ArrayTask(s) => write!(f, "{}", s),
        }
    }
}

impl Default for JobId {
    fn default() -> Self {
        JobId::Numeric(0)
    }
}

impl<'de> Deserialize<'de> for JobId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(JobId::Numeric(i))
                } else {
                    Ok(JobId::Numeric(0))
                }
            }
            serde_json::Value::String(s) => {
                if let Ok(n) = s.parse::<i64>() {
                    Ok(JobId::Numeric(n))
                } else {
                    Ok(JobId::ArrayTask(s))
                }
            }
            _ => Ok(JobId::Numeric(0)),
        }
    }
}

/// TRES (Trackable Resource) information
#[derive(Debug, Clone, Default)]
pub struct TresResource {
    pub res_type: String,
    pub name: String,
    pub id: i32,
    pub count: f64,
    pub task: Option<i32>,
    pub node: Option<String>,
}

/// TRES statistics with min/max/average/total
#[derive(Debug, Clone, Default)]
pub struct TresStats {
    pub max: Option<Vec<TresResource>>,
    pub min: Option<Vec<TresResource>>,
    pub average: Option<Vec<TresResource>>,
    pub total: Option<Vec<TresResource>>,
}

/// TRES data that can be either stats or a list of resources
#[derive(Debug, Clone)]
pub enum TresData {
    Stats(TresStats),
    Resources(Vec<TresResource>),
}

impl Default for TresData {
    fn default() -> Self {
        TresData::Resources(Vec::new())
    }
}

/// TRES allocation and usage information
#[derive(Debug, Clone, Default)]
pub struct TresInfo {
    pub requested: Option<TresData>,
    pub consumed: Option<TresData>,
    pub allocated: Option<Vec<TresResource>>,
}

/// Time information for jobs
#[derive(Debug, Clone, Default)]
pub struct TimeInfo {
    pub elapsed: i64,
    pub eligible: Option<i64>,
    pub end: i64,
    pub start: i64,
    pub submission: Option<i64>,
    pub suspended: i64,
    pub limit: Option<NumberValue>,
}

/// Number value that can be set or infinite
#[derive(Debug, Clone, Default)]
pub struct NumberValue {
    pub set: bool,
    pub infinite: bool,
    pub number: i64,
}

/// Job state information
#[derive(Debug, Clone, Default)]
pub struct State {
    pub current: Vec<String>,
    pub reason: String,
}

/// Exit code information
#[derive(Debug, Clone)]
pub struct ExitCode {
    pub status: Vec<String>,
    pub return_code: NumberValue,
}

impl Default for ExitCode {
    fn default() -> Self {
        Self {
            status: vec!["0".to_string()],
            return_code: NumberValue {
                set: true,
                infinite: false,
                number: 0,
            },
        }
    }
}

/// Job step information
#[derive(Debug, Clone, Default)]
pub struct JobStep {
    pub time: Option<TimeInfo>,
    pub exit_code: Option<ExitCode>,
    pub nodes: Option<HashMap<String, serde_json::Value>>,
    pub state: Option<Vec<String>>,
    pub tres: Option<TresInfo>,
}

/// Association information
#[derive(Debug, Clone, Default)]
pub struct Association {
    pub account: String,
    pub cluster: String,
    pub partition: String,
    pub user: String,
    pub id: i64,
}

/// Main Slurm job model
#[derive(Debug, Clone)]
pub struct SlurmJob {
    pub account: String,
    pub job_id: JobId,
    pub name: String,
    pub partition: String,
    pub state: State,
    pub time: TimeInfo,
    pub exit_code: ExitCode,
    pub tres: TresInfo,
    pub steps: Vec<JobStep>,
    pub association: Association,
    pub allocation_nodes: Option<i32>,
}

/// Calculated GPU metrics for a job
#[derive(Debug, Clone)]
pub struct GPUMetrics {
    pub user: String,
    pub job_id: JobId,
    pub state: String,
    pub elapsed: String,
    pub time_eff: String,
    pub cpu_eff: String,
    pub mem_eff: String,
    pub gpu_eff: String,
    pub gpu_mem: String,
    pub gpu_util: String,
    pub gpu_mem_eff: String,
    pub partition: String,
    pub account: Option<String>,
    pub node: Option<String>,
    pub gpu_type: Option<String>,
    pub gpu_count: i32,
}

/// Summary metrics for grouped jobs
#[derive(Debug, Clone)]
pub struct SummaryMetrics {
    pub user: String,
    pub partition: Option<String>,
    pub account: Option<String>,
    pub job_count: usize,
    pub total_gpu_hours: f64,
    pub avg_gpu_eff: f64,
    pub avg_gpu_mem_eff: f64,
    pub avg_time_eff: f64,
    pub avg_cpu_eff: f64,
    pub avg_mem_eff: f64,
    pub completed_jobs: usize,
    pub failed_jobs: usize,
    pub running_jobs: usize,
    pub pending_jobs: usize,
}

/// Node information from scontrol
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeInfo {
    pub name: String,
    #[serde(default)]
    pub gres: String,
    #[serde(default)]
    pub gres_used: String,
    pub state: Option<Vec<String>>,
    pub partitions: Option<Vec<String>>,
    #[serde(default)]
    pub cpus: i32,
    #[serde(default)]
    pub alloc_cpus: i32,
    #[serde(default)]
    pub alloc_idle_cpus: i32,
    #[serde(default)]
    pub cpu_load: f64,
}

/// Information about GPU slots on a node
#[derive(Debug, Clone)]
pub struct GPUSlotsInfo {
    pub gpu_type: String,
    pub total_count: i32,
    pub used_count: i32,
    pub node_name: String,
}

impl GPUSlotsInfo {
    pub fn utilization_percent(&self) -> f64 {
        if self.total_count == 0 {
            return 0.0;
        }
        (self.used_count as f64 / self.total_count as f64) * 100.0
    }
}

/// Summary of GPU usage by type
#[derive(Debug, Clone)]
pub struct GPUTypeSummary {
    pub gpu_type: String,
    pub total_gpus: i32,
    pub used_gpus: i32,
    pub available_gpus: i32,
    pub nodes_with_type: Vec<String>,
}

impl GPUTypeSummary {
    pub fn utilization_percent(&self) -> f64 {
        if self.total_gpus == 0 {
            return 0.0;
        }
        (self.used_gpus as f64 / self.total_gpus as f64) * 100.0
    }
}

/// Information about CPU slots on a node
#[derive(Debug, Clone)]
pub struct CPUSlotsInfo {
    pub total_cpus: i32,
    pub used_cpus: i32,
    pub available_cpus: i32,
    pub cpu_load: f64,
    pub node_name: String,
    pub partition: String,
}

impl CPUSlotsInfo {
    pub fn utilization_percent(&self) -> f64 {
        if self.total_cpus == 0 {
            return 0.0;
        }
        (self.used_cpus as f64 / self.total_cpus as f64) * 100.0
    }
}

/// Information about a queued job
#[derive(Debug, Clone)]
pub struct QueuedJobInfo {
    pub job_id: String,
    pub user: String,
    pub partition: String,
    pub state: String,
    pub cpu_request: i32,
    pub gpu_request: i32,
    pub gpu_type_request: Option<String>,
    pub queue_reason: String,
    pub submit_time: Option<String>,
    pub is_array_job: bool,
    pub array_task_count: i32,
    pub array_task_string: Option<String>,
    pub per_task_cpus: i32,
    pub per_task_gpus: i32,
}

/// Summary of CPU usage across nodes
#[derive(Debug, Clone)]
pub struct CPUTypeSummary {
    pub total_cpus: i32,
    pub used_cpus: i32,
    pub available_cpus: i32,
    pub avg_cpu_load: f64,
    pub nodes_with_cpus: Vec<String>,
}

impl CPUTypeSummary {
    pub fn utilization_percent(&self) -> f64 {
        if self.total_cpus == 0 {
            return 0.0;
        }
        (self.used_cpus as f64 / self.total_cpus as f64) * 100.0
    }
}

/// Summary of queued jobs
#[derive(Debug, Clone)]
pub struct QueueSummary {
    pub total_jobs: usize,
    pub total_cpus: i32,
    pub total_gpus: i32,
    pub jobs_by_user: HashMap<String, usize>,
    pub jobs_by_partition: HashMap<String, usize>,
    pub jobs_by_reason: HashMap<String, usize>,
}
