use std::process::Command;

use crate::command_ext::{run_with_timeout, SLURM_COMMAND_TIMEOUT};
use crate::models::NodeInfo;

pub(crate) struct ClusterDataCollector;

impl ClusterDataCollector {
    /// Get node data from scontrol show node command.
    pub fn get_node_data(debug: bool, partitions: Option<&[String]>) -> Vec<NodeInfo> {
        let cmd = ["scontrol", "show", "node", "--json"];

        if debug {
            eprintln!("Running command: {}", cmd.join(" "));
        }

        let mut cmd = Command::new("scontrol");
        cmd.args(["show", "node", "--json"]);
        let output = match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!("Error running scontrol: {}", stderr);
                return Vec::new();
            }
            Err(e) => {
                eprintln!("Error running scontrol: {}", e);
                return Vec::new();
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);

        if debug {
            eprintln!("Debug: scontrol returned {} bytes", stdout.len());
        }

        let data: serde_json::Value = match serde_json::from_str(&stdout) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Error parsing scontrol JSON output: {}", e);
                return Vec::new();
            }
        };

        let nodes_data = match data.get("nodes").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return Vec::new(),
        };

        if debug {
            eprintln!("Debug: Found {} nodes in JSON", nodes_data.len());
        }

        let mut nodes: Vec<NodeInfo> = Vec::new();
        for node_data in nodes_data {
            match serde_json::from_value::<NodeInfo>(node_data.clone()) {
                Ok(node) => nodes.push(node),
                Err(e) => {
                    if debug {
                        eprintln!("Warning: Failed to parse node: {}", e);
                    }
                }
            }
        }

        // Filter by partition if specified
        if let Some(partition_list) = partitions {
            let original_count = nodes.len();
            nodes.retain(|node| {
                if let Some(ref node_partitions) = node.partitions {
                    node_partitions.iter().any(|p| partition_list.contains(p))
                } else {
                    false
                }
            });

            if debug {
                eprintln!(
                    "Debug: Filtered {} nodes to {} nodes in partitions {:?}",
                    original_count,
                    nodes.len(),
                    partition_list
                );
            }
        }

        nodes
    }
}
