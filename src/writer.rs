use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::model::OptionValue;

/// Writes pending config changes back into a flake.nix file.
///
/// v1 strategy: generate / overwrite only the `configuration.nix` section
/// that lives alongside the flake, targeting the standard NixOS pattern:
///
///   /etc/nixos/flake.nix        ← minimal flake, imports ./configuration.nix
///   /etc/nixos/configuration.nix ← all option assignments live here
///
/// rnix AST-level rewriting is used to preserve any hand-written expressions
/// and comments outside the generated section.
pub struct FlakeWriter {
    pub config_dir: PathBuf,
}

impl FlakeWriter {
    pub fn new(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: config_dir.into(),
        }
    }

    /// Atomic write: write to a temp file then rename.
    pub async fn write_configuration(
        &self,
        changes: &HashMap<String, Option<OptionValue>>,
    ) -> Result<()> {
        let cfg_path = self.config_dir.join("configuration.nix");

        // Read existing file or start from a minimal skeleton.
        let existing = fs::read_to_string(&cfg_path).await.unwrap_or_default();

        let updated = apply_changes_to_source(&existing, changes)?;

        // Write atomically via temp file.
        let tmp_path = cfg_path.with_extension("nix.tmp");
        fs::write(&tmp_path, &updated)
            .await
            .context("failed to write temp configuration")?;
        fs::rename(&tmp_path, &cfg_path)
            .await
            .context("failed to rename temp configuration")?;

        Ok(())
    }

    /// Scaffold a minimal flake.nix if none exists.
    pub async fn scaffold_flake(&self, hostname: &str) -> Result<()> {
        let flake_path = self.config_dir.join("flake.nix");
        if fs::metadata(&flake_path).await.is_ok() {
            return Ok(()); // Don't overwrite existing flake.
        }

        let content = minimal_flake(hostname);
        fs::write(&flake_path, content)
            .await
            .context("failed to write flake.nix")?;

        let cfg_path = self.config_dir.join("configuration.nix");
        if fs::metadata(&cfg_path).await.is_err() {
            fs::write(&cfg_path, minimal_configuration(hostname))
                .await
                .context("failed to write configuration.nix")?;
        }

        Ok(())
    }

    /// Run `nixos-rebuild dry-run` to validate without applying.
    pub async fn dry_run(&self) -> Result<String> {
        let out = tokio::process::Command::new("nixos-rebuild")
            .args([
                "dry-run",
                "--flake",
                &format!("{}#", self.config_dir.display()),
            ])
            .output()
            .await
            .context("failed to run nixos-rebuild dry-run")?;

        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();

        if out.status.success() {
            Ok(format!("{stdout}\n{stderr}"))
        } else {
            Err(anyhow::anyhow!("nixos-rebuild dry-run failed:\n{stderr}"))
        }
    }

    /// Run `nixos-rebuild switch` to apply config.
    pub async fn apply(&self) -> Result<String> {
        let out = tokio::process::Command::new("nixos-rebuild")
            .args([
                "switch",
                "--flake",
                &format!("{}#", self.config_dir.display()),
            ])
            .output()
            .await
            .context("failed to run nixos-rebuild switch")?;

        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();

        if out.status.success() {
            Ok(format!("{stdout}\n{stderr}"))
        } else {
            Err(anyhow::anyhow!("nixos-rebuild switch failed:\n{stderr}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Source manipulation
// ---------------------------------------------------------------------------

/// Apply a map of option-name → new-value to a Nix configuration source string.
///
/// v1: We look for the `nixcfg-managed` block delimited by marker comments,
/// and regenerate only that section.  Everything outside the markers is
/// preserved verbatim (hand-written content, comments, etc.).
fn apply_changes_to_source(
    source: &str,
    changes: &HashMap<String, Option<OptionValue>>,
) -> Result<String> {
    const START: &str = "# nixcfg:start";
    const END: &str = "# nixcfg:end";

    let managed_block = generate_managed_block(changes);

    if source.contains(START) {
        // Replace existing managed block.
        let before = source.find(START).unwrap();
        let after = source
            .find(END)
            .map(|i| i + END.len())
            .unwrap_or(source.len());
        let result = format!(
            "{}{managed_block}{}",
            &source[..before],
            &source[after.min(source.len())..]
        );
        Ok(result)
    } else if source.trim().is_empty() {
        // No existing file — produce a full skeleton.
        Ok(minimal_configuration_with_block(&managed_block))
    } else {
        // Existing hand-written config — inject before the closing `}`.
        if let Some(last_brace) = source.rfind('}') {
            let result = format!(
                "{}\n  {}\n{}",
                &source[..last_brace],
                managed_block,
                &source[last_brace..]
            );
            Ok(result)
        } else {
            // Fallback: append.
            Ok(format!("{source}\n{managed_block}"))
        }
    }
}

fn generate_managed_block(changes: &HashMap<String, Option<OptionValue>>) -> String {
    let mut lines = Vec::new();
    lines.push("# nixcfg:start  (managed by nixcfg — do not edit this block manually)".into());

    let mut sorted: Vec<_> = changes.iter().collect();
    sorted.sort_by_key(|(k, _)| k.as_str());

    for (name, val_opt) in &sorted {
        match val_opt {
            Some(val) => {
                lines.push(format!("  {} = {};", name, value_to_nix(val)));
            }
            None => {
                // Explicitly reset to default — emit a comment.
                lines.push(format!("  # {} = <default>;", name));
            }
        }
    }

    lines.push("# nixcfg:end".into());
    lines.join("\n")
}

fn value_to_nix(val: &OptionValue) -> String {
    match val {
        OptionValue::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        OptionValue::Int(n) => n.to_string(),
        OptionValue::Float(f) => format!("{f}"),
        OptionValue::Str(s) => format!("\"{}\"", s.replace('"', "\\\"")),
        OptionValue::Null => "null".into(),
        OptionValue::List(items) => {
            let inner: Vec<String> = items.iter().map(value_to_nix).collect();
            format!("[ {} ]", inner.join(" "))
        }
        OptionValue::Attrs(map) => {
            let pairs: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{k} = {};", value_to_nix(v)))
                .collect();
            format!("{{ {} }}", pairs.join(" "))
        }
    }
}

// ---------------------------------------------------------------------------
// Scaffolding templates
// ---------------------------------------------------------------------------

fn minimal_flake(hostname: &str) -> String {
    format!(
        r#"{{
  description = "NixOS configuration managed by nixcfg";

  inputs = {{
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  }};

  outputs = {{ self, nixpkgs }}: {{
    nixosConfigurations.{hostname} = nixpkgs.lib.nixosSystem {{
      system = "x86_64-linux";
      modules = [ ./configuration.nix ];
    }};
  }};
}}
"#
    )
}

fn minimal_configuration(hostname: &str) -> String {
    format!(
        r#"{{ config, pkgs, ... }}:
{{
  networking.hostName = "{hostname}";
  system.stateVersion = "24.05";
}}
"#
    )
}

fn minimal_configuration_with_block(block: &str) -> String {
    format!(
        r#"{{ config, pkgs, ... }}:
{{
{block}
}}
"#
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_to_nix() {
        assert_eq!(value_to_nix(&OptionValue::Bool(true)), "true");
        assert_eq!(value_to_nix(&OptionValue::Bool(false)), "false");
    }

    #[test]
    fn list_to_nix() {
        let val = OptionValue::List(vec![OptionValue::Int(22), OptionValue::Int(80)]);
        assert_eq!(value_to_nix(&val), "[ 22 80 ]");
    }

    #[test]
    fn str_escapes_quotes() {
        let val = OptionValue::Str("say \"hello\"".into());
        assert_eq!(value_to_nix(&val), r#""say \"hello\"""#);
    }

    #[test]
    fn managed_block_round_trips() {
        let mut changes = HashMap::new();
        changes.insert(
            "services.openssh.enable".into(),
            Some(OptionValue::Bool(true)),
        );
        let source = "";
        let result = apply_changes_to_source(source, &changes).unwrap();
        assert!(result.contains("services.openssh.enable = true;"));
        assert!(result.contains("# nixcfg:start"));
        assert!(result.contains("# nixcfg:end"));
    }

    #[test]
    fn second_write_replaces_block() {
        let mut changes = HashMap::new();
        changes.insert(
            "networking.hostName".into(),
            Some(OptionValue::Str("mybox".into())),
        );
        let first = apply_changes_to_source("", &changes).unwrap();

        changes.insert(
            "networking.hostName".into(),
            Some(OptionValue::Str("newbox".into())),
        );
        let second = apply_changes_to_source(&first, &changes).unwrap();

        assert!(second.contains("\"newbox\""));
        assert!(!second.contains("\"mybox\""));
        // Only one managed block.
        assert_eq!(second.matches("# nixcfg:start").count(), 1);
    }
}
