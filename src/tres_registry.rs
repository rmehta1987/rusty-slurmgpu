use std::collections::HashMap;
use std::process::Command;

use crate::command_ext::{run_with_timeout, SLURM_COMMAND_TIMEOUT};

use once_cell::sync::Lazy;
use std::sync::Mutex;

static REGISTRY: Lazy<Mutex<TresRegistry>> = Lazy::new(|| Mutex::new(TresRegistry::new()));

pub struct TresRegistry {
    tres_map: HashMap<(String, String), i32>,
    loaded: bool,
}

impl TresRegistry {
    fn new() -> Self {
        Self {
            tres_map: HashMap::new(),
            loaded: false,
        }
    }

    /// Get singleton instance of TresRegistry.
    pub fn get_instance() -> TresRegistryRef {
        TresRegistryRef
    }

    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.load_tres_data();
    }

    fn load_tres_data(&mut self) {
        if self.loaded {
            return;
        }

        let mut cmd = Command::new("sacctmgr");
        cmd.args(["show", "tres", "--parsable2", "--noheader"]);
        match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut loaded_any = false;
                for line in stdout.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    // Format: Type|Name|ID  (Name may be empty → consecutive ||)
                    let parts: Vec<&str> = line.splitn(3, '|').collect();
                    if parts.len() < 3 {
                        continue;
                    }
                    let tres_type = parts[0].to_string();
                    let tres_name = parts[1].to_string();
                    let tres_id: i32 = match parts[2].parse() {
                        Ok(id) => id,
                        Err(_) => continue,
                    };
                    if !tres_type.is_empty() {
                        self.tres_map.insert((tres_type, tres_name), tres_id);
                        loaded_any = true;
                    }
                }
                if loaded_any {
                    self.loaded = true;
                    return;
                }
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!("Warning: sacctmgr failed: {}", stderr);
            }
            Err(e) => {
                eprintln!("Warning: Failed to run sacctmgr: {}", e);
            }
        }

        self.load_fallback_tres_data();
    }

    fn load_fallback_tres_data(&mut self) {
        let fallback = vec![
            ("cpu", "", 1),
            ("mem", "", 2),
            ("energy", "", 3),
            ("node", "", 4),
            ("billing", "", 5),
            ("gres", "gpu", 1001),
        ];

        for (tres_type, tres_name, tres_id) in fallback {
            self.tres_map
                .insert((tres_type.to_string(), tres_name.to_string()), tres_id);
        }

        self.loaded = true;
        eprintln!("Using fallback TRES IDs - some functionality may be limited");
    }
}

/// A handle to the singleton TresRegistry. All method calls go through the Mutex.
pub struct TresRegistryRef;

impl TresRegistryRef {
    pub fn get_tres_id(&self, tres_type: &str, tres_name: &str) -> Option<i32> {
        let mut registry = REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry.ensure_loaded();
        registry
            .tres_map
            .get(&(tres_type.to_string(), tres_name.to_string()))
            .copied()
    }

    pub fn get_gpu_tres_id(&self, gpu_type: &str) -> Option<i32> {
        if !gpu_type.is_empty() {
            let gpu_name = format!("gpu:{}", gpu_type);
            if let Some(id) = self.get_tres_id("gres", &gpu_name) {
                return Some(id);
            }
        }
        self.get_tres_id("gres", "gpu")
    }

    pub fn debug_print_tres_map(&self) {
        let mut registry = REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry.ensure_loaded();

        println!("TRES Registry Contents:");
        let mut entries: Vec<_> = registry.tres_map.iter().collect();
        entries.sort_by_key(|(k, _)| (k.0.clone(), k.1.clone()));

        for ((tres_type, tres_name), tres_id) in entries {
            let name_str = if tres_name.is_empty() {
                String::new()
            } else {
                format!(":{}", tres_name)
            };
            println!("  {}{} -> ID {}", tres_type, name_str, tres_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_tres_parsable2(input: &str) -> HashMap<(String, String), i32> {
        let mut map = HashMap::new();
        for line in input.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() < 3 {
                continue;
            }
            let tres_type = parts[0].to_string();
            let tres_name = parts[1].to_string();
            if let Ok(id) = parts[2].parse::<i32>() {
                if !tres_type.is_empty() {
                    map.insert((tres_type, tres_name), id);
                }
            }
        }
        map
    }

    #[test]
    fn test_parse_tres_parsable2() {
        let input = "cpu||1\nmem||2\nenergy||3\nnode||4\nbilling||5\nfs|disk|6\nvmem||7\npages||8\ngres|gpu|1001\n";
        let map = parse_tres_parsable2(input);
        assert_eq!(map.get(&("cpu".to_string(), "".to_string())), Some(&1));
        assert_eq!(map.get(&("gres".to_string(), "gpu".to_string())), Some(&1001));
        assert_eq!(map.get(&("fs".to_string(), "disk".to_string())), Some(&6));
        assert_eq!(map.len(), 9);
    }

    #[test]
    fn test_parse_tres_parsable2_with_header() {
        // --parsable2 without --noheader includes a Type|Name|ID header line
        let input = "Type|Name|ID\ncpu||1\ngres|gpu|1001\n";
        // The header line: "Type" has no integer in column 3 so it's skipped
        let map = parse_tres_parsable2(input);
        assert_eq!(map.len(), 2);
    }
}
