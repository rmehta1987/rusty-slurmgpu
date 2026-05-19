use std::process::Command;

use crate::command_ext::{run_with_timeout, SLURM_COMMAND_TIMEOUT};
use crate::models::NodeInfo;
use crate::scontrol_parser::{clean_value, parse_records};

/// GPU model tokens we recognise in AvailableFeatures / ActiveFeatures.
/// Matched case-insensitively against comma-separated feature tokens.
const KNOWN_GPU_TYPES: &[&str] = &[
    "a100", "h100", "h200", "v100", "rtx6000", "l40s", "a40", "t4", "a30",
];

/// Extract a GPU model name from a comma-separated Slurm features string.
/// Returns `None` if no known GPU type token is found.
pub(crate) fn gpu_type_from_features(features: &str) -> Option<String> {
    for token in features.split(',') {
        let lower = token.trim().to_lowercase();
        if KNOWN_GPU_TYPES.contains(&lower.as_str()) {
            // Preserve original casing so callers see e.g. "H200" → store as lowercase
            return Some(lower);
        }
    }
    None
}

/// Extract used GPU count from an AllocTRES string like `cpu=24,mem=256G,gres/gpu=4`.
/// Returns a synthetic gres string `gpu:N` compatible with GresParser, or empty string.
fn gres_used_from_alloc_tres(alloc_tres: &str) -> String {
    if alloc_tres.is_empty() {
        return String::new();
    }
    for part in alloc_tres.split(',') {
        if let Some((key, val)) = part.split_once('=') {
            if key == "gres/gpu" {
                if let Ok(n) = val.parse::<i32>() {
                    if n > 0 {
                        // Produce plain format: GresParser handles "gpu:N" via plain path
                        return format!("gpu:{}(IDX:0)", n);
                    }
                }
            }
        }
    }
    String::new()
}

pub(crate) struct ClusterDataCollector;

impl ClusterDataCollector {
    /// Get node data from scontrol show node command.
    pub fn get_node_data(debug: bool, partitions: Option<&[String]>) -> Vec<NodeInfo> {
        let cmd_str = "scontrol show node";
        if debug {
            eprintln!("Running command: {}", cmd_str);
        }

        let mut cmd = Command::new("scontrol");
        cmd.args(["show", "node"]);
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

        let records = parse_records(&stdout);

        if debug {
            eprintln!("Debug: Found {} nodes", records.len());
        }

        let mut nodes: Vec<NodeInfo> = Vec::new();
        for rec in &records {
            let name = match rec.get("NodeName").and_then(|s| clean_value(s)) {
                Some(n) => n.to_string(),
                None => continue,
            };

            let gres_raw = rec.get("Gres").map(|s| s.as_str()).unwrap_or("");
            let gres = if clean_value(gres_raw).is_some() {
                gres_raw.to_string()
            } else {
                String::new()
            };

            let alloc_tres = rec.get("AllocTRES").map(|s| s.as_str()).unwrap_or("");
            let gres_used = gres_used_from_alloc_tres(alloc_tres);

            let state_raw = rec.get("State").and_then(|s| clean_value(s));
            let state = state_raw.map(|s| vec![s.to_string()]);

            let partitions = rec
                .get("Partitions")
                .and_then(|s| clean_value(s))
                .map(|s| s.split(',').map(|p| p.to_string()).collect::<Vec<_>>());

            let cpu_tot: i32 = rec
                .get("CPUTot")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let cpu_alloc: i32 = rec
                .get("CPUAlloc")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let cpu_load: f64 = rec
                .get("CPULoad")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);

            let node = NodeInfo {
                name,
                gres,
                gres_used,
                state,
                partitions,
                cpus: cpu_tot,
                alloc_cpus: cpu_alloc,
                alloc_idle_cpus: cpu_tot - cpu_alloc,
                cpu_load,
            };

            if debug && node.gres.contains("gpu:") {
                eprintln!(
                    "Debug: Node {} gres={} gres_used={}",
                    node.name, node.gres, node.gres_used
                );
            }

            nodes.push(node);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_type_from_features_h200() {
        assert_eq!(
            gpu_type_from_features("gold-6542Y,1t,H200,DLC"),
            Some("h200".to_string())
        );
    }

    #[test]
    fn test_gpu_type_from_features_a100() {
        assert_eq!(
            gpu_type_from_features("gold-6248r,384g,a100"),
            Some("a100".to_string())
        );
    }

    #[test]
    fn test_gpu_type_from_features_none() {
        assert_eq!(gpu_type_from_features("Gold-6448Y,256g"), None);
    }

    #[test]
    fn test_gpu_type_from_features_rtx6000() {
        assert_eq!(
            gpu_type_from_features("gold-6248r,192g,rtx6000"),
            Some("rtx6000".to_string())
        );
    }

    #[test]
    fn test_gres_used_from_alloc_tres_with_gpu() {
        let result = gres_used_from_alloc_tres("cpu=24,mem=256G,gres/gpu=4");
        assert_eq!(result, "gpu:4(IDX:0)");
    }

    #[test]
    fn test_gres_used_from_alloc_tres_no_gpu() {
        let result = gres_used_from_alloc_tres("cpu=64,mem=257660M,billing=64");
        assert_eq!(result, "");
    }

    #[test]
    fn test_gres_used_from_alloc_tres_empty() {
        assert_eq!(gres_used_from_alloc_tres(""), "");
    }

    #[test]
    fn test_gres_used_from_alloc_tres_zero_gpu() {
        let result = gres_used_from_alloc_tres("cpu=48,mem=375G,gres/gpu=0");
        assert_eq!(result, "");
    }
}
