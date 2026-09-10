use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Per-compartment policy: permissions, resource limits, communication whitelist.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompartmentConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(
        default = "default_permissions",
        deserialize_with = "de_null_permissions"
    )]
    pub permissions: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
    #[serde(default)]
    pub memory_mb: u64,
    #[serde(default)]
    pub storage_mb: u64,
    #[serde(default)]
    pub cpu_percent: u64,
    #[serde(default = "default_wildcard", deserialize_with = "de_null_wildcard")]
    pub allow_inbound_from: Vec<String>,
    #[serde(default = "default_wildcard", deserialize_with = "de_null_wildcard")]
    pub allow_outbound_to: Vec<String>,
}

fn de_null_permissions<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(d)?.unwrap_or_else(default_permissions))
}

fn de_null_wildcard<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(d)?.unwrap_or_else(default_wildcard))
}

fn default_permissions() -> Vec<String> {
    vec!["fs_read".to_string()]
}

fn default_timeout() -> u64 {
    300
}

fn default_wildcard() -> Vec<String> {
    vec!["*".to_string()]
}

impl CompartmentConfig {
    pub fn is_unnamed(&self) -> bool {
        self.name.is_empty() || self.name == "unnamed"
    }
}

/// Coordinates compartment lifecycles and message routing rules.
pub struct Runtime {
    compartments: HashMap<String, CompartmentConfig>,
    order: Vec<String>,
    edges: Vec<(String, String)>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            compartments: HashMap::new(),
            order: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn add(&mut self, config: CompartmentConfig) -> Result<(), String> {
        if config.is_unnamed() {
            return Err("Every compartment needs a unique name in its config.".to_string());
        }
        if self.compartments.contains_key(&config.name) {
            return Err(format!(
                "Compartment '{}' is already registered.",
                config.name
            ));
        }
        self.order.push(config.name.clone());
        self.compartments.insert(config.name.clone(), config);
        Ok(())
    }

    pub fn edge(&mut self, from: &str, to: &str) -> Result<(), String> {
        let Some(src_cfg) = self.compartments.get(from) else {
            return Err(format!("Unknown source compartment: '{from}'"));
        };
        let Some(dst_cfg) = self.compartments.get(to) else {
            return Err(format!("Unknown target compartment: '{to}'"));
        };

        if dst_cfg.allow_inbound_from != ["*"]
            && !dst_cfg.allow_inbound_from.iter().any(|s| s == from)
        {
            return Err(format!(
                "Compartment '{to}' does not accept inbound from '{from}'. allow_inbound_from = {:?}",
                dst_cfg.allow_inbound_from
            ));
        }
        if src_cfg.allow_outbound_to != ["*"] && !src_cfg.allow_outbound_to.iter().any(|s| s == to)
        {
            return Err(format!(
                "Compartment '{from}' cannot outbound to '{to}'. allow_outbound_to = {:?}",
                src_cfg.allow_outbound_to
            ));
        }
        self.edges.push((from.to_string(), to.to_string()));
        Ok(())
    }

    // Registration order, optionally starting at `entry` (mirrors Python's run).
    pub fn run_order(&self, entry: Option<&str>) -> Result<Vec<String>, String> {
        if self.compartments.is_empty() {
            return Err("No compartments registered. Call add() first.".to_string());
        }
        let names: Vec<String> = self.order.clone();
        match entry {
            Some(e) => {
                if !self.compartments.contains_key(e) {
                    return Err(format!(
                        "Entry compartment '{e}' not found. Registered: {names:?}"
                    ));
                }
                let start = names.iter().position(|n| n == e).unwrap_or(0);
                Ok(names[start..].to_vec())
            }
            None => Ok(names),
        }
    }

    pub fn can_route(&self, from: &str, to: &str) -> Result<(), String> {
        let Some(src_cfg) = self.compartments.get(from) else {
            return Err(format!("Unknown source compartment: '{from}'"));
        };
        let Some(dst_cfg) = self.compartments.get(to) else {
            return Err(format!("Unknown target compartment: '{to}'"));
        };
        if dst_cfg.allow_inbound_from != ["*"]
            && !dst_cfg.allow_inbound_from.iter().any(|s| s == from)
        {
            return Err(format!(
                "Compartment '{to}' does not accept inbound from '{from}'. allow_inbound_from = {:?}",
                dst_cfg.allow_inbound_from
            ));
        }
        if src_cfg.allow_outbound_to != ["*"] && !src_cfg.allow_outbound_to.iter().any(|s| s == to)
        {
            return Err(format!(
                "Compartment '{from}' cannot outbound to '{to}'. allow_outbound_to = {:?}",
                src_cfg.allow_outbound_to
            ));
        }
        Ok(())
    }

    pub fn config(&self, name: &str) -> Option<&CompartmentConfig> {
        self.compartments.get(name)
    }

    pub fn names(&self) -> Vec<String> {
        self.order.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(name: &str) -> CompartmentConfig {
        CompartmentConfig {
            name: name.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn add_requires_unique_name() {
        let mut rt = Runtime::new();
        rt.add(cfg("a")).unwrap();
        assert!(rt.add(cfg("a")).is_err());
        assert!(rt.add(CompartmentConfig::default()).is_err());
    }

    #[test]
    fn edge_validates_whitelists() {
        let mut rt = Runtime::new();
        rt.add(cfg("a")).unwrap();
        let mut b = cfg("b");
        b.allow_inbound_from = vec!["c".to_string()];
        rt.add(b).unwrap();
        assert!(rt.edge("a", "b").is_err());
    }

    #[test]
    fn can_route_respects_outbound() {
        let mut rt = Runtime::new();
        let mut a = cfg("a");
        a.allow_outbound_to = vec!["c".to_string()];
        rt.add(a).unwrap();
        rt.add(cfg("b")).unwrap();
        assert!(rt.can_route("a", "b").is_err());
        assert!(rt.can_route("a", "c").is_err()); // c not registered
    }

    #[test]
    fn serde_tolerates_null_vec_fields() {
        // Go/TS SDKs emit `null` for nil slices - must not error.
        let cfg: CompartmentConfig =
            serde_json::from_str(r#"{"name":"a","permissions":null,"allow_inbound_from":null}"#)
                .unwrap();
        assert_eq!(cfg.permissions, vec!["fs_read"]);
        assert_eq!(cfg.allow_inbound_from, vec!["*"]);
        assert_eq!(cfg.allow_outbound_to, vec!["*"]);
        assert_eq!(cfg.timeout_s, 300);
    }
}
