use std::collections::HashMap;

use chrono::{Local, NaiveDateTime, TimeZone};

/// Parse scontrol text output into a list of key→value records.
///
/// Records are separated by blank lines. Each line is tokenised on whitespace;
/// each token is split on the FIRST `=` only, so values like
/// `TRES=cpu=16,mem=500G` are stored correctly.  Tokens without `=` are
/// skipped.  The sentinel values `(null)`, `N/A`, `None`, and empty string are
/// kept as-is; callers should use [`clean_value`] to normalise them.
pub(crate) fn parse_records(input: &str) -> Vec<HashMap<String, String>> {
    let mut records: Vec<HashMap<String, String>> = Vec::new();
    let mut current: HashMap<String, String> = HashMap::new();

    for line in input.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                records.push(std::mem::take(&mut current));
            }
            continue;
        }
        for token in line.split_whitespace() {
            if let Some(eq_pos) = token.find('=') {
                let key = &token[..eq_pos];
                let value = &token[eq_pos + 1..];
                if !key.is_empty() {
                    current.insert(key.to_string(), value.to_string());
                }
            }
        }
    }

    if !current.is_empty() {
        records.push(current);
    }

    records
}

/// Return `Some(s)` unless `s` is one of the Slurm "no value" sentinels.
pub(crate) fn clean_value(s: &str) -> Option<&str> {
    match s {
        "" | "(null)" | "N/A" | "None" | "NONE" | "UNLIMITED" => None,
        other => Some(other),
    }
}

/// Parse a Slurm time-limit string to total minutes.
///
/// Formats handled:
/// - `D-HH:MM:SS`
/// - `HH:MM:SS` / `H:MM:SS`
/// - `MM:SS`
/// - Bare integer (already in minutes)
/// - `UNLIMITED`, `NONE`, `N/A`, `(null)` → `None`
pub(crate) fn parse_duration_minutes(s: &str) -> Option<i64> {
    match s {
        "" | "UNLIMITED" | "NONE" | "N/A" | "(null)" | "None" => return None,
        _ => {}
    }

    // D-HH:MM:SS
    if let Some(dash_pos) = s.find('-') {
        let days: i64 = s[..dash_pos].parse().ok()?;
        let rest = &s[dash_pos + 1..];
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 3 {
            let h: i64 = parts[0].parse().ok()?;
            let m: i64 = parts[1].parse().ok()?;
            let sec: i64 = parts[2].parse().ok()?;
            return Some(days * 1440 + h * 60 + m + sec / 60);
        }
        return None;
    }

    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        // HH:MM:SS or H:MM:SS
        3 => {
            let h: i64 = parts[0].parse().ok()?;
            let m: i64 = parts[1].parse().ok()?;
            let sec: i64 = parts[2].parse().ok()?;
            Some(h * 60 + m + sec / 60)
        }
        // MM:SS
        2 => {
            let m: i64 = parts[0].parse().ok()?;
            Some(m)
        }
        // bare integer already in minutes
        1 => s.parse().ok(),
        _ => None,
    }
}

/// Parse a Slurm ISO datetime string (`YYYY-MM-DDTHH:MM:SS`) to epoch seconds.
///
/// Treats the datetime as local time.  Returns `None` for `N/A`, `(null)`,
/// `None`, `Unknown`, or anything that does not parse.
pub(crate) fn parse_datetime_epoch(s: &str) -> Option<i64> {
    match s {
        "" | "N/A" | "(null)" | "None" | "Unknown" => return None,
        _ => {}
    }
    let naive = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()?;
    let local = Local.from_local_datetime(&naive).single()?;
    Some(local.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPU_NODE: &str = r"NodeName=midway3-0604 Arch=x86_64 CoresPerSocket=24
   CPUAlloc=24 CPUTot=48 CPULoad=4.20
   AvailableFeatures=gold-6542Y,1t,H200,DLC
   ActiveFeatures=gold-6542Y,1t,H200,DLC
   Gres=gpu:4
   NodeAddr=midway3-0604 NodeHostName=midway3-0604 Version=20.11.8
   RealMemory=1031795 AllocMem=262144 FreeMem=1009091 Sockets=2 Boards=1
   State=MIXED ThreadsPerCore=1 TmpDisk=0 Weight=1 Owner=N/A MCS_label=N/A
   Partitions=gagalli-gpu
   CfgTRES=cpu=48,mem=1031795M,billing=48,gres/gpu=4
   AllocTRES=cpu=24,mem=256G,gres/gpu=4
   Comment=(null)";

    const NON_GPU_NODE: &str = r"NodeName=midway3-0398 Arch=x86_64 CoresPerSocket=32
   CPUAlloc=0 CPUTot=64 CPULoad=0.01
   AvailableFeatures=Gold-6448Y,256g
   ActiveFeatures=Gold-6448Y,256g
   Gres=(null)
   State=IDLE ThreadsPerCore=1 TmpDisk=0 Weight=1 Owner=N/A MCS_label=N/A
   Partitions=jjberg
   CfgTRES=cpu=64,mem=257660M,billing=64
   AllocTRES=
   Comment=(null)";

    #[test]
    fn test_parse_gpu_node() {
        let records = parse_records(GPU_NODE);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.get("NodeName").map(|s| s.as_str()), Some("midway3-0604"));
        assert_eq!(r.get("CPUTot").map(|s| s.as_str()), Some("48"));
        assert_eq!(r.get("CPUAlloc").map(|s| s.as_str()), Some("24"));
        assert_eq!(r.get("Gres").map(|s| s.as_str()), Some("gpu:4"));
        assert_eq!(r.get("Partitions").map(|s| s.as_str()), Some("gagalli-gpu"));
        assert_eq!(r.get("State").map(|s| s.as_str()), Some("MIXED"));
        assert_eq!(r.get("AllocTRES").map(|s| s.as_str()), Some("cpu=24,mem=256G,gres/gpu=4"));
    }

    #[test]
    fn test_parse_non_gpu_node() {
        let records = parse_records(NON_GPU_NODE);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.get("Gres").map(|s| s.as_str()), Some("(null)"));
        assert_eq!(r.get("AllocTRES").map(|s| s.as_str()), Some(""));
    }

    #[test]
    fn test_parse_two_records() {
        let input = format!("{}\n\n{}", GPU_NODE, NON_GPU_NODE);
        let records = parse_records(&input);
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_clean_value() {
        assert_eq!(clean_value(""), None);
        assert_eq!(clean_value("(null)"), None);
        assert_eq!(clean_value("N/A"), None);
        assert_eq!(clean_value("None"), None);
        assert_eq!(clean_value("NONE"), None);
        assert_eq!(clean_value("UNLIMITED"), None);
        assert_eq!(clean_value("midway3-0604"), Some("midway3-0604"));
        assert_eq!(clean_value("MIXED"), Some("MIXED"));
    }

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(parse_duration_minutes("3-00:00:00"), Some(3 * 1440));
        assert_eq!(parse_duration_minutes("10-00:00:00"), Some(10 * 1440));
        assert_eq!(parse_duration_minutes("1-12:00:00"), Some(1440 + 12 * 60));
    }

    #[test]
    fn test_parse_duration_hms() {
        assert_eq!(parse_duration_minutes("02:00:00"), Some(120));
        assert_eq!(parse_duration_minutes("4:00:00"), Some(240));
        assert_eq!(parse_duration_minutes("7-00:00:00"), Some(7 * 1440));
    }

    #[test]
    fn test_parse_duration_mm_ss() {
        assert_eq!(parse_duration_minutes("30:00"), Some(30));
    }

    #[test]
    fn test_parse_duration_sentinels() {
        assert_eq!(parse_duration_minutes("UNLIMITED"), None);
        assert_eq!(parse_duration_minutes("NONE"), None);
        assert_eq!(parse_duration_minutes("N/A"), None);
        assert_eq!(parse_duration_minutes("(null)"), None);
        assert_eq!(parse_duration_minutes(""), None);
    }

    #[test]
    fn test_parse_datetime_valid() {
        let epoch = parse_datetime_epoch("2026-05-11T13:32:02");
        assert!(epoch.is_some());
        assert!(epoch.unwrap() > 1_700_000_000);
    }

    #[test]
    fn test_parse_datetime_sentinels() {
        assert_eq!(parse_datetime_epoch("N/A"), None);
        assert_eq!(parse_datetime_epoch("(null)"), None);
        assert_eq!(parse_datetime_epoch(""), None);
    }
}
