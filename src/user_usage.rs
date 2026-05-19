use std::collections::HashMap;
use std::process::Command;

use crate::command_ext::{run_with_timeout, SLURM_COMMAND_TIMEOUT};
use crate::parser::SlurmJobParser;
use crate::slurm_utils::build_node_gpu_mapping;
use crate::tres_parser::TresParser;

/// Combined resource usage data for all users.
pub(crate) struct UserResourceUsage {
    /// user -> {gpu_type: count}
    pub(crate) gpu_usage: HashMap<String, HashMap<String, i32>>,
    /// user -> cpu_count
    pub(crate) cpu_usage: HashMap<String, i32>,
    /// user -> memory_mb
    pub(crate) memory_usage: HashMap<String, i64>,
}

pub(crate) struct UserUsageAnalyzer;

impl UserUsageAnalyzer {
    /// Get GPU usage by type for a specific user.
    /// Returns: HashMap mapping GPU type to (gpu_count, node_count)
    pub fn get_user_gpu_usage(user: &str, debug: bool) -> HashMap<String, (i32, usize)> {
        let mut gpu_usage: HashMap<String, i32> = HashMap::new();
        let mut gpu_nodes: HashMap<String, std::collections::HashSet<String>> = HashMap::new();

        let mut cmd = Command::new("squeue");
        cmd.args([
            "-O",
            "jobid,username,state,nodelist:50,tres-alloc:200",
            "-h",
            "-u",
            user,
            "-t",
            "RUNNING",
        ]);

        match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);

                // Parse squeue output
                let mut jobs_data = Vec::new();
                for line in stdout.trim().lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 5 {
                        let jobid = parts[0];
                        let username = parts[1];
                        let state = parts[2];
                        let nodelist = parts[3];
                        let tres_alloc = parts[4..].join(" ");
                        jobs_data.push(format!(
                            "{},{},{},{},{}",
                            jobid, username, state, nodelist, tres_alloc
                        ));
                    }
                }

                let csv_output = jobs_data.join("\n");
                let parsed_jobs = SlurmJobParser::parse_squeue_output(&csv_output);

                let node_gpu_map = build_node_gpu_mapping(false);

                for job in &parsed_jobs {
                    let tres_req_str = job.get("tres_req_str").map(|s| s.as_str()).unwrap_or("");
                    let nodes_str = job.get("nodes").map(|s| s.as_str()).unwrap_or("");

                    if debug {
                        let job_id = job.get("job_id").map(|s| s.as_str()).unwrap_or("");
                        if !tres_req_str.is_empty() {
                            eprintln!("Debug: Job {} TRES: {}", job_id, tres_req_str);
                        }
                    }

                    let (gpu_count, gpu_type_from_tres) =
                        SlurmJobParser::extract_gpu_info_from_tres(tres_req_str);

                    if gpu_count > 0 {
                        let node_name = parse_node_name(nodes_str);

                        let gpu_type = if gpu_type_from_tres != "unknown" {
                            gpu_type_from_tres
                        } else if let Some(ref nn) = node_name {
                            node_gpu_map
                                .get(nn)
                                .cloned()
                                .unwrap_or_else(|| "unknown".to_string())
                        } else {
                            "unknown".to_string()
                        };

                        if let Some(ref nn) = node_name {
                            gpu_nodes
                                .entry(gpu_type.clone())
                                .or_default()
                                .insert(nn.clone());
                        }
                        *gpu_usage.entry(gpu_type).or_insert(0) += gpu_count;
                    }
                }
            }
            Ok(output) => {
                if debug {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("Warning: Failed to get user jobs: {}", stderr);
                }
            }
            Err(e) => {
                if debug {
                    eprintln!("Warning: Error getting user GPU usage: {}", e);
                }
            }
        }

        // Convert to final format
        let mut result = HashMap::new();
        for (gpu_type, count) in &gpu_usage {
            let node_count = gpu_nodes.get(gpu_type).map_or(0, |s| s.len());
            result.insert(gpu_type.clone(), (*count, node_count));
        }

        if debug {
            eprintln!("Debug: User {} GPU usage: {:?}", user, result);
        }

        result
    }

    /// Get total CPU usage for a specific user.
    pub fn get_user_cpu_usage(user: &str, debug: bool) -> i32 {
        let mut cpu_usage = 0;

        let mut cmd = Command::new("squeue");
        cmd.args(["-O", "jobid,numcpus", "-h", "-u", user, "-t", "RUNNING"]);

        match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.trim().lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(cpus) = parts[1].parse::<i32>() {
                            cpu_usage += cpus;
                        }
                    }
                }

                if debug {
                    eprintln!("Debug: User {} CPU usage: {}", user, cpu_usage);
                }
            }
            Ok(output) => {
                if debug {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("Warning: Failed to get user CPU usage: {}", stderr);
                }
            }
            Err(e) => {
                if debug {
                    eprintln!("Warning: Error getting user CPU usage: {}", e);
                }
            }
        }

        cpu_usage
    }

    /// Get GPU, CPU, and memory usage for all users from a single squeue call.
    pub fn get_all_users_resource_usage(
        debug: bool,
        partitions: Option<&[String]>,
    ) -> UserResourceUsage {
        let mut user_gpu_usage: HashMap<String, HashMap<String, i32>> = HashMap::new();
        let mut user_cpu_usage: HashMap<String, i32> = HashMap::new();
        let mut user_memory_usage: HashMap<String, i64> = HashMap::new();

        let mut cmd = Command::new("squeue");
        cmd.args([
            "-O",
            "jobid,username,state,nodelist:50,tres-alloc:200",
            "-h",
            "-t",
            "RUNNING",
        ]);

        if let Some(parts) = partitions {
            let partition_str = parts.join(",");
            cmd.args(["-p", &partition_str]);
        }

        if debug {
            eprintln!("Debug: Running squeue for all users resource usage");
        }

        match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);

                let mut jobs_data = Vec::new();
                for line in stdout.trim().lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 5 {
                        let jobid = parts[0];
                        let username = parts[1];
                        let state = parts[2];
                        let nodelist = parts[3];
                        let tres_alloc = parts[4..].join(" ");
                        jobs_data.push(format!(
                            "{},{},{},{},{}",
                            jobid, username, state, nodelist, tres_alloc
                        ));
                    }
                }

                let csv_output = jobs_data.join("\n");
                let parsed_jobs = SlurmJobParser::parse_squeue_output(&csv_output);
                let node_gpu_map = build_node_gpu_mapping(false);

                for job in &parsed_jobs {
                    let username = job
                        .get("user_name")
                        .map(|s| s.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let tres_req_str = job.get("tres_req_str").map(|s| s.as_str()).unwrap_or("");
                    let nodes_str = job.get("nodes").map(|s| s.as_str()).unwrap_or("");

                    if tres_req_str.is_empty() {
                        continue;
                    }

                    // Parse TRES string once
                    let resources = TresParser::parse_tres_string(tres_req_str);

                    // Extract GPU info
                    let (gpu_count, gpu_type_from_tres) =
                        SlurmJobParser::extract_gpu_info_from_tres(tres_req_str);

                    if gpu_count > 0 {
                        let node_name = parse_node_name(nodes_str);

                        let gpu_type = if gpu_type_from_tres != "unknown" {
                            gpu_type_from_tres
                        } else if let Some(ref nn) = node_name {
                            node_gpu_map
                                .get(nn)
                                .cloned()
                                .unwrap_or_else(|| "unknown".to_string())
                        } else {
                            "unknown".to_string()
                        };

                        *user_gpu_usage
                            .entry(username.clone())
                            .or_default()
                            .entry(gpu_type)
                            .or_insert(0) += gpu_count;
                    }

                    // Extract CPU count from TRES
                    for resource in &resources {
                        if resource.name == "cpu" {
                            *user_cpu_usage.entry(username.clone()).or_insert(0) +=
                                resource.count as i32;
                            break;
                        }
                    }

                    // Extract memory from TRES
                    let memory_mb = TresParser::extract_memory_mb(&resources);
                    if memory_mb > 0.0 {
                        *user_memory_usage.entry(username).or_insert(0) += memory_mb as i64;
                    }
                }

                if debug {
                    eprintln!(
                        "Debug: Found {} users with GPU, {} with CPU, {} with memory",
                        user_gpu_usage.len(),
                        user_cpu_usage.len(),
                        user_memory_usage.len()
                    );
                }
            }
            Ok(output) => {
                if debug {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("Warning: Failed to get user resource usage: {}", stderr);
                }
            }
            Err(e) => {
                if debug {
                    eprintln!("Warning: Error getting user resource usage: {}", e);
                }
            }
        }

        UserResourceUsage {
            gpu_usage: user_gpu_usage,
            cpu_usage: user_cpu_usage,
            memory_usage: user_memory_usage,
        }
    }
}

/// Parse first node name from nodelist string.
fn parse_node_name(nodes_str: &str) -> Option<String> {
    if nodes_str.is_empty() {
        return None;
    }

    if nodes_str.contains('[') {
        // Format: "prefix[num1,num2]"
        let prefix = nodes_str.split('[').next().unwrap_or("");
        let first_num = nodes_str
            .split('[')
            .nth(1)
            .unwrap_or("")
            .split(',')
            .next()
            .unwrap_or("")
            .split(']')
            .next()
            .unwrap_or("");
        Some(format!("{}{}", prefix, first_num))
    } else {
        Some(nodes_str.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_node_name_simple() {
        assert_eq!(parse_node_name("linux46"), Some("linux46".to_string()));
    }

    #[test]
    fn test_parse_node_name_range() {
        assert_eq!(parse_node_name("linux[51,55]"), Some("linux51".to_string()));
    }

    #[test]
    fn test_parse_node_name_empty() {
        assert_eq!(parse_node_name(""), None);
    }
}
