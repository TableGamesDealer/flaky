use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;

use crate::OptionValue;

/// Reads an existing flake.nix and extracts the current values of known options.
///
/// Strategy (v1): shell out to `nix eval --json` on each option path we care
/// about.  This is slower than a full AST walk but is robust to any valid
/// Nix syntax.  A future version can use rnix for instant offline parsing.
pub struct FlakeParser {
    pub flake_path: PathBuf,
    /// Hostname to use when evaluating nixosConfigurations.<host>.
    pub hostname: Option<String>,
}

impl FlakeParser {
    pub fn new(flake_path: impl Into<PathBuf>) -> Self {
        Self {
            flake_path: flake_path.into(),
            hostname: None,
        }
    }

    pub fn with_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Return true if a flake.nix exists at `flake_path`.
    pub async fn exists(&self) -> bool {
        let p = self.flake_path.join("flake.nix");
        fs::metadata(&p).await.is_ok()
    }

    /// Read the raw source text of flake.nix.
    pub async fn read_source(&self) -> Result<String> {
        let p = self.flake_path.join("flake.nix");
        fs::read_to_string(&p)
            .await
            .with_context(|| format!("could not read {}", p.display()))
    }

    /// Parse the current option values by evaluating each option path.
    /// Returns a map of option-name → current value.
    ///
    /// In demo mode (no nix binary), returns an empty map so the TUI
    /// falls back to option defaults.
    pub async fn load_current_values(
        &self,
        option_names: &[String],
    ) -> Result<HashMap<String, OptionValue>> {
        let host = self.effective_hostname().await?;
        let mut values = HashMap::new();

        for name in option_names {
            let expr = format!(
                r#"(builtins.getFlake "{flake}").nixosConfigurations."{host}".config.{name}"#,
                flake = self.flake_path.display(),
                host = host,
                name = name,
            );

            let result = tokio::process::Command::new("nix")
                .args(["eval", "--json", "--expr", &expr])
                .output()
                .await;

            match result {
                Ok(out) if out.status.success() => {
                    if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&out.stdout)
                        && let Some(ov) = json_to_option_value(&val)
                    {
                        values.insert(name.clone(), ov);
                    }
                }
                // Missing / unevaluatable options are silently skipped;
                // the TUI will show the schema default instead.
                _ => {}
            }
        }

        Ok(values)
    }

    /// Detect the hostname from the flake's nixosConfigurations outputs.
    async fn effective_hostname(&self) -> Result<String> {
        if let Some(ref h) = self.hostname {
            return Ok(h.clone());
        }

        // Try to read it from the flake outputs.
        let expr = format!(
            r#"builtins.attrNames (builtins.getFlake "{}").nixosConfigurations"#,
            self.flake_path.display()
        );

        let out = tokio::process::Command::new("nix")
            .args(["eval", "--json", "--expr", &expr])
            .output()
            .await;

        match out {
            Ok(o) if o.status.success() => {
                let names: Vec<String> = serde_json::from_slice(&o.stdout).unwrap_or_default();
                names
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("no nixosConfigurations found in flake"))
            }
            _ => {
                // No nix available or flake not yet initialised — use machine hostname.
                let name = "flaky_machine".to_owned();
                Ok(name)
            }
        }
    }
}

fn json_to_option_value(val: &serde_json::Value) -> Option<OptionValue> {
    use serde_json::Value;
    match val {
        Value::Bool(b) => Some(OptionValue::Bool(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(OptionValue::Int(i))
            } else {
                n.as_f64().map(OptionValue::Float)
            }
        }
        Value::String(s) => Some(OptionValue::Str(s.clone())),
        Value::Null => Some(OptionValue::Null),
        Value::Array(arr) => Some(OptionValue::List(
            arr.iter().filter_map(json_to_option_value).collect(),
        )),
        Value::Object(obj) => {
            let mut map = indexmap::IndexMap::new();
            for (k, v) in obj {
                if let Some(ov) = json_to_option_value(v) {
                    map.insert(k.clone(), ov);
                }
            }
            Some(OptionValue::Attrs(map))
        }
    }
}
