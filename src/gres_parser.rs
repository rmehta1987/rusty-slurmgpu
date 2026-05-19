use once_cell::sync::Lazy;
use regex::Regex;

// Matches typed (gpu:TYPE:COUNT(flags)) and typeless (gpu:COUNT(flags)) entries.
static GPU_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"gpu:[^,]+\([^)]*\)").expect("GPU_PATTERN regex is valid"));
// Captures optional TYPE and required COUNT. When TYPE is absent the cluster has no
// autodetect=nvml and no explicit type name in gres.conf.
static GPU_DETAILS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"gpu:(?:([^:,\(]+):)?(\d+)\([^)]*\)")
        .expect("GPU_DETAILS_PATTERN regex is valid")
});

// Plain-text scontrol show node emits Gres=gpu:4 with no parenthetical flags.
// Matches optional TYPE and required COUNT with no trailing flags.
static GPU_PLAIN_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:^|,)gpu:(?:([^:,\s]+):)?(\d+)(?:,|$)")
        .expect("GPU_PLAIN_PATTERN regex is valid")
});

pub(crate) struct GresParser;

impl GresParser {
    /// Parse GRES string to extract GPU types and total counts.
    ///
    /// Handles both the parenthetical format from scontrol JSON / gres.conf
    /// (`gpu:TYPE:COUNT(flags)`) and the plain format from scontrol text output
    /// (`gpu:COUNT` or `gpu:TYPE:COUNT`).
    pub(crate) fn parse_gres_string(gres: &str) -> Vec<(String, i32)> {
        let mut gpu_info = Vec::new();
        if gres.is_empty() || !gres.contains("gpu:") {
            return gpu_info;
        }

        // Try the parenthetical format first (JSON / gres.conf autodetect)
        let paren_matches: Vec<_> = GPU_PATTERN.find_iter(gres).collect();
        if !paren_matches.is_empty() {
            for mat in paren_matches {
                let part = mat.as_str().trim();
                if !part.starts_with("gpu:") {
                    continue;
                }
                if let Some(cap) = GPU_DETAILS_PATTERN.captures(part) {
                    let gpu_type = cap.get(1).map(|m| m.as_str()).unwrap_or("gpu");
                    let count: i32 = cap[2].parse().unwrap_or(0);
                    gpu_info.push((gpu_type.to_string(), count));
                }
            }
            return gpu_info;
        }

        // Fall back to plain format: gpu:N or gpu:TYPE:N
        // Prepend/append comma so the anchored regex matches at start/end too.
        let anchored = format!(",{},", gres);
        for cap in GPU_PLAIN_PATTERN.captures_iter(&anchored) {
            let gpu_type = cap.get(1).map(|m| m.as_str()).unwrap_or("gpu");
            let count: i32 = cap[2].parse().unwrap_or(0);
            gpu_info.push((gpu_type.to_string(), count));
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
                let gpu_type = cap.get(1).map(|m| m.as_str()).unwrap_or("gpu");
                let count: i32 = cap[2].parse().unwrap_or(0);
                gpu_used.push((gpu_type.to_string(), count));
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
        let result = GresParser::parse_gres_string("gpu:a6000:8(S:0-1),gpu:rtx_2080:2(S:2)");
        assert_eq!(
            result,
            vec![("a6000".to_string(), 8), ("rtx_2080".to_string(), 2),]
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
        let result = GresParser::parse_gres_string("gpu:nvidia_a100-sxm4-80gb:4(S:0-1)");
        assert_eq!(result, vec![("nvidia_a100-sxm4-80gb".to_string(), 4)]);
    }

    #[test]
    fn test_parse_gres_mig_slice() {
        let result = GresParser::parse_gres_string("gpu:h200_1g.18gb:2(S:0)");
        assert_eq!(result, vec![("h200_1g.18gb".to_string(), 2)]);
    }

    // Typeless GRES — clusters without autodetect=nvml and no explicit type in gres.conf
    #[test]
    fn test_parse_gres_typeless_single() {
        let result = GresParser::parse_gres_string("gpu:4(S:0-1)");
        assert_eq!(result, vec![("gpu".to_string(), 4)]);
    }

    #[test]
    fn test_parse_gres_typeless_multiple() {
        let result = GresParser::parse_gres_string("gpu:4(S:0-1),gpu:4(S:2-3)");
        assert_eq!(result, vec![("gpu".to_string(), 4), ("gpu".to_string(), 4)]);
    }

    #[test]
    fn test_parse_gres_used_typeless() {
        let result = GresParser::parse_gres_used_string("gpu:2(IDX:0,1)");
        assert_eq!(result, vec![("gpu".to_string(), 2)]);
    }

    #[test]
    fn test_parse_gres_used_typeless_zero() {
        let result = GresParser::parse_gres_used_string("gpu:0(IDX:N/A)");
        assert_eq!(result, vec![("gpu".to_string(), 0)]);
    }

    // Plain text format from scontrol show node (no parenthetical flags)
    #[test]
    fn test_parse_gres_plain_typeless() {
        let result = GresParser::parse_gres_string("gpu:4");
        assert_eq!(result, vec![("gpu".to_string(), 4)]);
    }

    #[test]
    fn test_parse_gres_plain_with_type() {
        let result = GresParser::parse_gres_string("gpu:a100:4");
        assert_eq!(result, vec![("a100".to_string(), 4)]);
    }

    #[test]
    fn test_parse_gres_plain_null() {
        let result = GresParser::parse_gres_string("(null)");
        assert!(result.is_empty());
    }
}
