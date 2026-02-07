use std::collections::{BTreeSet, HashMap};

use crate::models::{
    CPUSlotsInfo, CPUTypeSummary, GPUSlotsInfo, GPUTypeSummary, QueueSummary, QueuedJobInfo,
};

pub struct ResourceSummarizer;

impl ResourceSummarizer {
    /// Summarize GPU usage by type across all nodes.
    pub fn summarize_by_gpu_type(gpu_slots_list: &[GPUSlotsInfo]) -> Vec<GPUTypeSummary> {
        let mut type_data: HashMap<String, (i32, i32, BTreeSet<String>)> = HashMap::new();

        for slot in gpu_slots_list {
            let entry = type_data
                .entry(slot.gpu_type.clone())
                .or_insert_with(|| (0, 0, BTreeSet::new()));
            entry.0 += slot.total_count;
            entry.1 += slot.used_count;
            entry.2.insert(slot.node_name.clone());
        }

        let mut summaries: Vec<GPUTypeSummary> = type_data
            .into_iter()
            .map(|(gpu_type, (total, used, nodes))| GPUTypeSummary {
                gpu_type,
                total_gpus: total,
                used_gpus: used,
                available_gpus: total - used,
                nodes_with_type: nodes.into_iter().collect(),
            })
            .collect();

        summaries.sort_by(|a, b| a.gpu_type.cmp(&b.gpu_type));
        summaries
    }

    /// Summarize CPU usage across all nodes.
    pub fn summarize_cpu_usage(cpu_slots_list: &[CPUSlotsInfo]) -> CPUTypeSummary {
        if cpu_slots_list.is_empty() {
            return CPUTypeSummary {
                total_cpus: 0,
                used_cpus: 0,
                available_cpus: 0,
                avg_cpu_load: 0.0,
                nodes_with_cpus: Vec::new(),
            };
        }

        let total_cpus: i32 = cpu_slots_list.iter().map(|s| s.total_cpus).sum();
        let used_cpus: i32 = cpu_slots_list.iter().map(|s| s.used_cpus).sum();
        let available_cpus: i32 = cpu_slots_list.iter().map(|s| s.available_cpus).sum();

        // Calculate weighted average CPU load
        let total_load: f64 = cpu_slots_list
            .iter()
            .map(|s| s.cpu_load * s.total_cpus as f64)
            .sum();
        let avg_cpu_load = if total_cpus > 0 {
            total_load / total_cpus as f64
        } else {
            0.0
        };

        CPUTypeSummary {
            total_cpus,
            used_cpus,
            available_cpus,
            avg_cpu_load,
            nodes_with_cpus: cpu_slots_list.iter().map(|s| s.node_name.clone()).collect(),
        }
    }

    /// Summarize queue information from pending jobs.
    pub fn summarize_queue_info(queued_jobs: &[QueuedJobInfo]) -> QueueSummary {
        if queued_jobs.is_empty() {
            return QueueSummary {
                total_jobs: 0,
                total_cpus: 0,
                total_gpus: 0,
                jobs_by_user: HashMap::new(),
                jobs_by_partition: HashMap::new(),
                jobs_by_reason: HashMap::new(),
            };
        }

        let total_cpus: i32 = queued_jobs.iter().map(|j| j.cpu_request).sum();
        let total_gpus: i32 = queued_jobs.iter().map(|j| j.gpu_request).sum();

        let mut jobs_by_user: HashMap<String, usize> = HashMap::new();
        let mut jobs_by_partition: HashMap<String, usize> = HashMap::new();
        let mut jobs_by_reason: HashMap<String, usize> = HashMap::new();

        for job in queued_jobs {
            *jobs_by_user.entry(job.user.clone()).or_insert(0) += 1;
            *jobs_by_partition.entry(job.partition.clone()).or_insert(0) += 1;
            *jobs_by_reason.entry(job.queue_reason.clone()).or_insert(0) += 1;
        }

        QueueSummary {
            total_jobs: queued_jobs.len(),
            total_cpus,
            total_gpus,
            jobs_by_user,
            jobs_by_partition,
            jobs_by_reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarize_by_gpu_type() {
        let slots = vec![
            GPUSlotsInfo {
                gpu_type: "a6000".to_string(),
                total_count: 8,
                used_count: 4,
                node_name: "node1".to_string(),
            },
            GPUSlotsInfo {
                gpu_type: "a6000".to_string(),
                total_count: 8,
                used_count: 6,
                node_name: "node2".to_string(),
            },
            GPUSlotsInfo {
                gpu_type: "v100".to_string(),
                total_count: 4,
                used_count: 2,
                node_name: "node3".to_string(),
            },
        ];

        let summaries = ResourceSummarizer::summarize_by_gpu_type(&slots);
        assert_eq!(summaries.len(), 2);

        let a6000 = summaries.iter().find(|s| s.gpu_type == "a6000").unwrap();
        assert_eq!(a6000.total_gpus, 16);
        assert_eq!(a6000.used_gpus, 10);
        assert_eq!(a6000.available_gpus, 6);
        assert_eq!(a6000.nodes_with_type.len(), 2);

        let v100 = summaries.iter().find(|s| s.gpu_type == "v100").unwrap();
        assert_eq!(v100.total_gpus, 4);
        assert_eq!(v100.used_gpus, 2);
    }

    #[test]
    fn test_summarize_cpu_usage() {
        let slots = vec![
            CPUSlotsInfo {
                total_cpus: 64,
                used_cpus: 32,
                available_cpus: 32,
                cpu_load: 50.0,
                node_name: "node1".to_string(),
                partition: "gpu".to_string(),
            },
            CPUSlotsInfo {
                total_cpus: 64,
                used_cpus: 48,
                available_cpus: 16,
                cpu_load: 75.0,
                node_name: "node2".to_string(),
                partition: "gpu".to_string(),
            },
        ];

        let summary = ResourceSummarizer::summarize_cpu_usage(&slots);
        assert_eq!(summary.total_cpus, 128);
        assert_eq!(summary.used_cpus, 80);
        assert_eq!(summary.available_cpus, 48);
        assert_eq!(summary.nodes_with_cpus.len(), 2);
    }

    #[test]
    fn test_summarize_cpu_usage_empty() {
        let summary = ResourceSummarizer::summarize_cpu_usage(&[]);
        assert_eq!(summary.total_cpus, 0);
        assert_eq!(summary.used_cpus, 0);
    }

    #[test]
    fn test_summarize_queue_info() {
        let jobs = vec![
            QueuedJobInfo {
                job_id: "123".to_string(),
                user: "alice".to_string(),
                partition: "gpu".to_string(),
                state: "PENDING".to_string(),
                cpu_request: 4,
                gpu_request: 1,
                gpu_type_request: None,
                queue_reason: "Resources".to_string(),
                submit_time: None,
                is_array_job: false,
                array_task_count: 1,
                array_task_string: None,
                per_task_cpus: 4,
                per_task_gpus: 1,
            },
            QueuedJobInfo {
                job_id: "124".to_string(),
                user: "bob".to_string(),
                partition: "gpu".to_string(),
                state: "PENDING".to_string(),
                cpu_request: 8,
                gpu_request: 2,
                gpu_type_request: None,
                queue_reason: "Priority".to_string(),
                submit_time: None,
                is_array_job: false,
                array_task_count: 1,
                array_task_string: None,
                per_task_cpus: 8,
                per_task_gpus: 2,
            },
        ];

        let summary = ResourceSummarizer::summarize_queue_info(&jobs);
        assert_eq!(summary.total_jobs, 2);
        assert_eq!(summary.total_cpus, 12);
        assert_eq!(summary.total_gpus, 3);
        assert_eq!(summary.jobs_by_user.get("alice"), Some(&1));
        assert_eq!(summary.jobs_by_user.get("bob"), Some(&1));
    }

    #[test]
    fn test_summarize_queue_info_empty() {
        let summary = ResourceSummarizer::summarize_queue_info(&[]);
        assert_eq!(summary.total_jobs, 0);
        assert_eq!(summary.total_cpus, 0);
        assert_eq!(summary.total_gpus, 0);
    }
}
