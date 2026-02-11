use once_cell::sync::Lazy;
use regex::Regex;

static GPU_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"gpu:[^:,]+:\d+\([^)]*\)").expect("GPU_PATTERN regex is valid"));
static GPU_DETAILS_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"gpu:([^:,]+):(\d+)\([^)]*\)").expect("GPU_DETAILS_PATTERN regex is valid"));

pub(crate) struct GresParser;

impl GresParser {
    /// Parse GRES string to extract GPU types and total counts.
    pub(crate) fn parse_gres_string(gres: &str) -> Vec<(String, i32)> {
        let mut gpu_info = Vec::new();
        if gres.is_empty() || !gres.contains("gpu:") {
            return gpu_info;
        }

        for mat in GPU_PATTERN.find_iter(gres) {
            let part = mat.as_str().trim();
            if !part.starts_with("gpu:") {
                continue;
            }

            if let Some(cap) = GPU_DETAILS_PATTERN.captures(part) {
                let gpu_type = &cap[1];
                let count: i32 = cap[2].parse().unwrap_or(0);

                if !gpu_type.is_empty() && gpu_type != "gpu" {
                    gpu_info.push((gpu_type.to_string(), count));
                }
            }
        }

        gpu_info
    }

    /// Parse GRES used string to extract GPU types and used counts.
    pub(crate) fn parse_gres_used_string(gres_used: &str) -> Vec<(String, i32)> {
        let mut gpu_used = Vec::new();
        if gres_used.is_empty() || !gres_used.contains("gpu:") {
            return gpu_used;
        }

        for mat in GPU_PATTERN.find_iter(gres_used) {
            let part = mat.as_str().trim();
            if !part.starts_with("gpu:") {
                continue;
            }

            if let Some(cap) = GPU_DETAILS_PATTERN.captures(part) {
                let gpu_type = &cap[1];
                let count: i32 = cap[2].parse().unwrap_or(0);

                if !gpu_type.is_empty() && gpu_type != "gpu" {
                    gpu_used.push((gpu_type.to_string(), count));
                }
            }
        }

        gpu_used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gres_single_gpu() {
        let result = GresParser::parse_gres_string("gpu:p100:4(S:0-1)");
        assert_eq!(result, vec![("p100".to_string(), 4)]);
    }

    #[test]
    fn test_parse_gres_multiple_gpus() {
        let result =
            GresParser::parse_gres_string("gpu:a6000:8(S:0-1),gpu:rtx_2080:2(S:2)");
        assert_eq!(
            result,
            vec![
                ("a6000".to_string(), 8),
                ("rtx_2080".to_string(), 2),
            ]
        );
    }

    #[test]
    fn test_parse_gres_no_gpu() {
        let result = GresParser::parse_gres_string("no_gpu_here");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_gres_empty() {
        let result = GresParser::parse_gres_string("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_gres_used_string() {
        let result = GresParser::parse_gres_used_string("gpu:p100:4(IDX:0-3)");
        assert_eq!(result, vec![("p100".to_string(), 4)]);
    }

    #[test]
    fn test_parse_gres_used_zero() {
        let result = GresParser::parse_gres_used_string("gpu:a6000:0(IDX:N/A)");
        assert_eq!(result, vec![("a6000".to_string(), 0)]);
    }

    #[test]
    fn test_parse_gres_complex_name() {
        let result =
            GresParser::parse_gres_string("gpu:nvidia_a100-sxm4-80gb:4(S:0-1)");
        assert_eq!(result, vec![("nvidia_a100-sxm4-80gb".to_string(), 4)]);
    }

    #[test]
    fn test_parse_gres_mig_slice() {
        let result =
            GresParser::parse_gres_string("gpu:h200_1g.18gb:2(S:0)");
        assert_eq!(result, vec![("h200_1g.18gb".to_string(), 2)]);
    }
}
