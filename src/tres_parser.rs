use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};

use crate::models::TresResource;
use crate::tres_registry::TresRegistry;

static GPU_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"gres/gpu(?::([^=,]+))?=(\d+)").expect("GPU_PATTERN regex is valid"));
static MEMORY_UNITS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(\d+(?:\.\d+)?)([KMGT]?)$").expect("MEMORY_UNITS regex is valid"));
static GENERAL_TRES: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([^=,]+)=([^,]+)").expect("GENERAL_TRES regex is valid"));

static KNOWN_GPU_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "a100",
        "nvidia_a100-sxm4-80gb",
        "a40",
        "a5000",
        "a6000",
        "6000",
        "6000_ada",
        "v100",
        "p100",
        "k80",
        "rtx8000",
        "rtx_2080",
        "2080rtx",
        "2080",
        "rtx_5000",
        "5000_ada",
        "titan_v",
        "h100",
        "h200",
        "h200_1g.18gb",
        "h200_3g.71gb",
        "h200_4g.71gb",
        "rtx_pro_6000",
        "rtx_6000_pro",
        "6000_pro",
        "default",
    ]
    .into_iter()
    .collect()
});

pub(crate) struct TresParser;

impl TresParser {
    /// Parse a TRES string into structured TresResource objects.
    pub(crate) fn parse_tres_string(tres_str: &str) -> Vec<TresResource> {
        if tres_str.is_empty() {
            return Vec::new();
        }

        let mut resources = Vec::new();
        resources.extend(Self::parse_gpu_resources(tres_str));
        resources.extend(Self::parse_standard_resources(tres_str));
        resources
    }

    fn parse_gpu_resources(tres_str: &str) -> Vec<TresResource> {
        let mut resources = Vec::new();

        for cap in GPU_PATTERN.captures_iter(tres_str) {
            let gpu_type = cap.get(1).map_or("gpu", |m| m.as_str());
            let count_str = &cap[2];

            let count = match count_str.parse::<f64>() {
                Ok(c) => c,
                Err(_) => continue,
            };

            let registry = TresRegistry::get_instance();
            let gpu_key = if gpu_type == "gpu" { "" } else { gpu_type };
            let tres_id = registry.get_gpu_tres_id(gpu_key).unwrap_or(1004);

            resources.push(TresResource {
                res_type: "gres".to_string(),
                name: gpu_type.to_string(),
                id: tres_id,
                count,
                task: None,
                node: None,
            });
        }

        resources
    }

    fn parse_standard_resources(tres_str: &str) -> Vec<TresResource> {
        let mut resources = Vec::new();

        // Remove GPU patterns to avoid double-parsing
        let tres_no_gpu = GPU_PATTERN.replace_all(tres_str, "");

        for cap in GENERAL_TRES.captures_iter(&tres_no_gpu) {
            let resource_type = cap[1].trim();
            let value = cap[2].trim();

            if resource_type.starts_with("gres/gpu") {
                continue;
            }

            let registry = TresRegistry::get_instance();

            let (res_type, resource_id, count_value) = match resource_type {
                "cpu" => {
                    let id = registry.get_tres_id("cpu", "").unwrap_or(1);
                    match value.parse::<f64>() {
                        Ok(v) => ("cpu", id, v),
                        Err(_) => continue,
                    }
                }
                "mem" => {
                    let id = registry.get_tres_id("mem", "").unwrap_or(2);
                    ("mem", id, Self::parse_memory_value(value))
                }
                "node" => {
                    let id = registry.get_tres_id("node", "").unwrap_or(4);
                    match value.parse::<f64>() {
                        Ok(v) => ("node", id, v),
                        Err(_) => continue,
                    }
                }
                "billing" => {
                    let id = registry.get_tres_id("billing", "").unwrap_or(5);
                    match value.parse::<f64>() {
                        Ok(v) => ("billing", id, v),
                        Err(_) => continue,
                    }
                }
                _ => match value.parse::<f64>() {
                    Ok(v) => ("generic", 1000, v),
                    Err(_) => continue,
                },
            };

            resources.push(TresResource {
                res_type: res_type.to_string(),
                name: resource_type.to_string(),
                id: resource_id,
                count: count_value,
                task: None,
                node: None,
            });
        }

        resources
    }

    /// Extract total GPU count from TRES resources.
    /// In Slurm TRES, `gres/gpu=2,gres/gpu:p100=2` means 2 GPUs total (both p100).
    /// The generic `gres/gpu=N` is a summary; specific entries are the breakdown.
    /// If specific GPU type entries exist, count only those to avoid double-counting.
    pub(crate) fn extract_gpu_count(resources: &[TresResource]) -> i32 {
        let mut generic_count = 0;
        let mut specific_count = 0;

        for resource in resources {
            if resource.res_type == "gres" && Self::is_gpu_resource(&resource.name) {
                if resource.name == "gpu" {
                    generic_count += resource.count as i32;
                } else {
                    specific_count += resource.count as i32;
                }
            }
        }

        if specific_count > 0 {
            specific_count
        } else {
            generic_count
        }
    }

    /// Extract GPU types and their counts from TRES resources.
    /// Skips the generic "gpu" entry when specific GPU type entries exist,
    /// since Slurm TRES includes both as summary + breakdown (not additive).
    #[allow(dead_code)]
    pub(crate) fn extract_gpu_types(resources: &[TresResource]) -> HashMap<String, i32> {
        let mut gpu_types = HashMap::new();
        let has_specific = resources
            .iter()
            .any(|r| r.res_type == "gres" && Self::is_gpu_resource(&r.name) && r.name != "gpu");

        for resource in resources {
            if resource.res_type == "gres" && Self::is_gpu_resource(&resource.name) {
                // Skip generic "gpu" entry when specific types exist
                if resource.name == "gpu" && has_specific {
                    continue;
                }
                let gpu_type = if resource.name == "gpu" {
                    "default".to_string()
                } else {
                    resource.name.clone()
                };
                *gpu_types.entry(gpu_type).or_insert(0) += resource.count as i32;
            }
        }

        gpu_types
    }

    /// Extract memory allocation in MB from TRES resources.
    pub(crate) fn extract_memory_mb(resources: &[TresResource]) -> f64 {
        for resource in resources {
            if resource.name == "mem" {
                return resource.count;
            }
        }
        0.0
    }

    /// Parse memory string with optional units to MB.
    pub(crate) fn parse_memory_value(mem_str: &str) -> f64 {
        if mem_str.is_empty() {
            return 0.0;
        }

        if let Some(cap) = MEMORY_UNITS.captures(mem_str) {
            let value: f64 = cap[1].parse().unwrap_or(0.0);
            let unit = cap.get(2).map_or("M", |m| {
                let s = m.as_str();
                if s.is_empty() {
                    "M"
                } else {
                    s
                }
            });

            let multiplier = match unit {
                "K" => 1.0 / 1024.0,
                "M" => 1.0,
                "G" => 1024.0,
                "T" => 1024.0 * 1024.0,
                _ => 1.0,
            };

            return value * multiplier;
        }

        // Try plain number (assume MB)
        mem_str.parse::<f64>().unwrap_or(0.0)
    }

    /// Format memory in MB to human-readable string.
    pub(crate) fn format_memory_value(memory_mb: f64) -> String {
        if memory_mb <= 0.0 {
            return "-".to_string();
        }

        if memory_mb >= 1024.0 * 1024.0 {
            let value = memory_mb / (1024.0 * 1024.0);
            if (value - value.round()).abs() < 0.01 {
                format!("{}T", value as i64)
            } else {
                format!("{:.1}T", value)
            }
        } else if memory_mb >= 1024.0 {
            let value = memory_mb / 1024.0;
            if (value - value.round()).abs() < 0.01 {
                format!("{}G", value as i64)
            } else {
                format!("{:.1}G", value)
            }
        } else {
            format!("{}M", memory_mb.round() as i64)
        }
    }

    /// Check if a resource name represents a GPU.
    pub(crate) fn is_gpu_resource(name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        if name == "gpu" {
            return true;
        }
        if name.to_lowercase().contains("gpu") {
            return true;
        }
        KNOWN_GPU_TYPES.contains(name.to_lowercase().as_str())
    }

    /// Parse TRES consumed/requested string into metrics.
    #[allow(dead_code)]
    pub(crate) fn parse_tres_consumed(tres_str: &str) -> HashMap<String, f64> {
        let mut metrics = HashMap::new();
        metrics.insert("gpu_util".to_string(), 0.0);
        metrics.insert("gpu_mem_mb".to_string(), 0.0);

        if tres_str.is_empty() {
            return metrics;
        }

        for component in tres_str.split(',') {
            let parts: Vec<&str> = component.splitn(2, '=').collect();
            if parts.len() != 2 {
                continue;
            }

            let key = parts[0].trim();
            let value = parts[1].trim();

            match key {
                "gres/gpu" | "gres/gpu:util" => {
                    if let Ok(util_value) = value.parse::<f64>() {
                        let adjusted = if util_value > 1_000_000.0 {
                            util_value / 10_000_000.0
                        } else {
                            util_value
                        };
                        let current = *metrics.get("gpu_util").unwrap_or(&0.0);
                        metrics.insert("gpu_util".to_string(), current.max(adjusted));
                    }
                }
                "gres/gpu:mem" | "gres/gpumem" => {
                    let mem_mb = Self::parse_memory_value(value);
                    let current = *metrics.get("gpu_mem_mb").unwrap_or(&0.0);
                    metrics.insert("gpu_mem_mb".to_string(), current.max(mem_mb));
                }
                _ => {}
            }
        }

        metrics
    }

    /// Combine multiple TRES resource lists, aggregating counts.
    #[allow(dead_code)]
    pub(crate) fn combine_tres_resources(resource_lists: &[&[TresResource]]) -> Vec<TresResource> {
        let mut combined: HashMap<(String, String), TresResource> = HashMap::new();

        for resources in resource_lists {
            for resource in *resources {
                let key = (resource.res_type.clone(), resource.name.clone());
                let entry = combined.entry(key).or_insert_with(|| TresResource {
                    res_type: resource.res_type.clone(),
                    name: resource.name.clone(),
                    id: resource.id,
                    count: 0.0,
                    task: None,
                    node: None,
                });
                entry.count += resource.count;
            }
        }

        combined.into_values().collect()
    }

    /// Extract GPU type from TRES resources.
    /// Prefers a specific GPU type (e.g. "a5000") over the generic "gpu" entry.
    pub(crate) fn extract_gpu_type_from_tres(resources: &[TresResource]) -> Option<String> {
        let mut has_generic_gpu = false;

        // First pass: look for a specific GPU type
        for resource in resources {
            if resource.res_type == "gres" && !resource.name.is_empty() {
                if resource.name == "gpu" {
                    has_generic_gpu = true;
                } else if KNOWN_GPU_TYPES.contains(resource.name.to_lowercase().as_str()) {
                    return Some(resource.name.clone());
                } else if resource.name.to_lowercase().contains("gpu") {
                    if resource.name.contains(':') {
                        return Some(
                            resource
                                .name
                                .split_once(':')
                                .map(|x| x.1)
                                .unwrap_or("gpu")
                                .to_string(),
                        );
                    }
                    return Some(resource.name.clone());
                }
            }
        }

        // Fall back to generic "gpu" if no specific type found
        if has_generic_gpu {
            return Some("gpu".to_string());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tres_string_basic() {
        let resources = TresParser::parse_tres_string("cpu=4,mem=16G,node=1,billing=4");
        assert!(resources.iter().any(|r| r.name == "cpu" && r.count == 4.0));
        assert!(resources
            .iter()
            .any(|r| r.name == "mem" && (r.count - 16384.0).abs() < 0.1));
    }

    #[test]
    fn test_parse_tres_string_with_gpu() {
        let resources = TresParser::parse_tres_string("cpu=4,mem=16G,gres/gpu:a100=2");
        let gpu_count = TresParser::extract_gpu_count(&resources);
        assert_eq!(gpu_count, 2);
    }

    #[test]
    fn test_parse_memory_value() {
        assert!((TresParser::parse_memory_value("16G") - 16384.0).abs() < 0.1);
        assert!((TresParser::parse_memory_value("1024M") - 1024.0).abs() < 0.1);
        assert!((TresParser::parse_memory_value("1048576K") - 1024.0).abs() < 0.1);
        assert!((TresParser::parse_memory_value("1T") - 1048576.0).abs() < 0.1);
        assert!((TresParser::parse_memory_value("512") - 512.0).abs() < 0.1);
    }

    #[test]
    fn test_format_memory_value() {
        assert_eq!(TresParser::format_memory_value(102400.0), "100G");
        assert_eq!(TresParser::format_memory_value(1572864.0), "1.5T");
        assert_eq!(TresParser::format_memory_value(512.0), "512M");
        assert_eq!(TresParser::format_memory_value(0.0), "-");
    }

    #[test]
    fn test_is_gpu_resource() {
        assert!(TresParser::is_gpu_resource("gpu"));
        assert!(TresParser::is_gpu_resource("a100"));
        assert!(TresParser::is_gpu_resource("v100"));
        assert!(!TresParser::is_gpu_resource("cpu"));
        assert!(!TresParser::is_gpu_resource("mem"));
        assert!(!TresParser::is_gpu_resource(""));
    }

    #[test]
    fn test_extract_gpu_types() {
        let resources = TresParser::parse_tres_string("gres/gpu:a100=2,gres/gpu:v100=1");
        let types = TresParser::extract_gpu_types(&resources);
        assert_eq!(types.get("a100"), Some(&2));
        assert_eq!(types.get("v100"), Some(&1));
    }

    #[test]
    fn test_gpu_count_no_double_counting() {
        // Slurm TRES has both generic gres/gpu=N and specific gres/gpu:type=N
        // These are summary + breakdown, NOT additive
        let resources = TresParser::parse_tres_string("cpu=1,mem=30G,gres/gpu=1,gres/gpu:p100=1");
        let gpu_count = TresParser::extract_gpu_count(&resources);
        assert_eq!(
            gpu_count, 1,
            "should not double-count generic + specific GPU entries"
        );

        let resources = TresParser::parse_tres_string("cpu=1,gres/gpu=2,gres/gpu:rtx_pro_6000=2");
        let gpu_count = TresParser::extract_gpu_count(&resources);
        assert_eq!(gpu_count, 2);

        let types = TresParser::extract_gpu_types(&resources);
        assert_eq!(types.get("rtx_pro_6000"), Some(&2));
        assert_eq!(
            types.get("default"),
            None,
            "generic gpu should be excluded when specific types exist"
        );
    }

    #[test]
    fn test_gpu_count_generic_only() {
        // When only generic gres/gpu entry exists, use that count
        let resources = TresParser::parse_tres_string("cpu=4,gres/gpu=3");
        let gpu_count = TresParser::extract_gpu_count(&resources);
        assert_eq!(gpu_count, 3);
    }
}
