use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;

use crate::command_ext::{run_with_timeout, SLURM_COMMAND_TIMEOUT};

use once_cell::sync::Lazy;

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
        cmd.args(["show", "tres", "--json"]);
        match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT)
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                match serde_json::from_str::<serde_json::Value>(&stdout) {
                    Ok(data) => {
                        if let Some(tres_list) = data.get("TRES").and_then(|v| v.as_array()) {
                            for tres in tres_list {
                                let tres_type = tres
                                    .get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let tres_name = tres
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let tres_id = tres
                                    .get("id")
                                    .and_then(|v| v.as_i64())
                                    .map(|v| v as i32);

                                if !tres_type.is_empty() {
                                    if let Some(id) = tres_id {
                                        self.tres_map
                                            .insert((tres_type, tres_name), id);
                                    }
                                }
                            }
                            self.loaded = true;
                            return;
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to parse TRES data: {}", e);
                    }
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
        let mut registry = REGISTRY.lock().unwrap();
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
        let mut registry = REGISTRY.lock().unwrap();
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
