use std::collections::HashMap;

use crate::cluster_data::ClusterDataCollector;
use crate::gres_parser::GresParser;
use crate::models::{CPUSlotsInfo, GPUSlotsInfo};

pub(crate) struct ResourceSlotCollector;

impl ResourceSlotCollector {
    /// Collect GPU slot information from all nodes.
    pub fn collect_gpu_slots_info(debug: bool, partitions: Option<&[String]>) -> Vec<GPUSlotsInfo> {
        let nodes = ClusterDataCollector::get_node_data(debug, partitions);
        let mut gpu_slots_list = Vec::new();

        for node in &nodes {
            if debug && (node.gres.contains("gpu:") || node.gres_used.contains("gpu:")) {
                eprintln!("Debug: Node {}", node.name);
                eprintln!("  GRES: {}", node.gres);
                eprintln!("  GRES_USED: {}", node.gres_used);
            }

            // Parse total GPU counts by type
            let total_gpus = GresParser::parse_gres_string(&node.gres);
            let used_gpus = GresParser::parse_gres_used_string(&node.gres_used);

            // Create a dict for easy lookup of used counts
            let used_dict: HashMap<String, i32> = used_gpus.into_iter().collect();

            // Create GPUSlotsInfo objects for each GPU type on this node
            for (gpu_type, total_count) in &total_gpus {
                let used_count = *used_dict.get(gpu_type).unwrap_or(&0);
                gpu_slots_list.push(GPUSlotsInfo {
                    gpu_type: gpu_type.clone(),
                    total_count: *total_count,
                    used_count,
                    node_name: node.name.clone(),
                });
            }
        }

        gpu_slots_list
    }

    /// Collect CPU slot information from all nodes.
    pub fn collect_cpu_slots_info(debug: bool, partitions: Option<&[String]>) -> Vec<CPUSlotsInfo> {
        let nodes = ClusterDataCollector::get_node_data(debug, partitions);
        let mut cpu_slots_list = Vec::new();

        for node in &nodes {
            if debug {
                eprintln!(
                    "Debug: Node {} - CPUs: {}, Used: {}, Load: {}",
                    node.name, node.cpus, node.alloc_cpus, node.cpu_load
                );
            }

            let partition_str = node
                .partitions
                .as_ref()
                .map(|p| p.join(","))
                .unwrap_or_else(|| "unknown".to_string());

            cpu_slots_list.push(CPUSlotsInfo {
                total_cpus: node.cpus,
                used_cpus: node.alloc_cpus,
                available_cpus: node.alloc_idle_cpus,
                cpu_load: node.cpu_load,
                node_name: node.name.clone(),
                partition: partition_str,
            });
        }

        cpu_slots_list
    }
}
