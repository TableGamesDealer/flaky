use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
use tokio::process::Command;

use crate::model::{NixOption, OptionType, OptionValue};

/// Drives `nix eval` to extract the NixOS option declarations for a given
/// flake or the system-installed nixpkgs.
pub struct NixEvaluator {
    /// Path to the `nix` binary.
    pub nix_bin: String,
}

impl NixEvaluator {
    pub fn new() -> Self {
        Self {
            nix_bin: "nix".into(),
        }
    }

    /// Check that `nix` is available on PATH.
    pub async fn probe(&self) -> Result<String> {
        let out = Command::new(&self.nix_bin)
            .args(["--version"])
            .output()
            .await
            .context("could not run `nix --version` — is Nix installed?")?;
        let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(version)
    }

    /// Extract the full NixOS option tree by evaluating:
    ///   nix eval --json <nixpkgs>/nixos --apply 'nixos: nixos.options'
    ///
    /// This is the same approach the NixOS options search uses.
    /// Returns a list of options parsed from the JSON output.
    pub async fn load_system_options(&self, flake_path: Option<&Path>) -> Result<Vec<NixOption>> {
        let expr = if let Some(path) = flake_path {
            // Evaluate options from the user's flake.
            format!(
                r#"(import {path}/flake.nix).nixosConfigurations.${{builtins.head (builtins.attrNames (import {path}/flake.nix).nixosConfigurations)}}.options"#,
                path = path.display()
            )
        } else {
            // Fallback: bare nixpkgs options (useful for demo / no flake yet).
            r#"(import <nixpkgs/nixos> { configuration = {}; }).options"#.into()
        };

        let raw = self.eval_json(&expr).await?;
        self.parse_options_json(raw)
    }

    /// Run `nix eval --json` with the given expression and return the parsed JSON value.
    pub async fn eval_json(&self, expr: &str) -> Result<Value> {
        let out = Command::new(&self.nix_bin)
            .args(["eval", "--json", "--expr", expr])
            .output()
            .await
            .context("failed to spawn `nix eval`")?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            bail!("nix eval failed:\n{stderr}");
        }

        serde_json::from_slice(&out.stdout).context("nix eval produced invalid JSON")
    }

    /// Walk the raw options JSON and produce a flat list of NixOptions.
    ///
    /// The NixOS options JSON has the shape:
    ///   { "services.openssh.enable": { "_type": "option", "type": {...}, ... }, ... }
    fn parse_options_json(&self, val: Value) -> Result<Vec<NixOption>> {
        let mut options = Vec::new();
        self.walk("", &val, &mut options);
        Ok(options)
    }

    fn walk(&self, prefix: &str, val: &Value, out: &mut Vec<NixOption>) {
        let Some(obj) = val.as_object() else { return };

        // Is this an option leaf?
        if obj.get("_type").and_then(|t| t.as_str()) == Some("option") {
            if let Some(opt) = self.parse_option_leaf(prefix, obj) {
                out.push(opt);
            }
            return;
        }

        // Otherwise it's a namespace — recurse.
        for (key, child) in obj {
            let full = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            self.walk(&full, child, out);
        }
    }

    fn parse_option_leaf(
        &self,
        name: &str,
        obj: &serde_json::Map<String, Value>,
    ) -> Option<NixOption> {
        let description = obj
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();

        let option_type = obj
            .get("type")
            .map(|t| parse_type(t))
            .unwrap_or(OptionType::Unknown {
                type_str: "?".into(),
            });

        let default = obj.get("default").map(json_to_value).flatten();
        let example = obj.get("example").map(json_to_value).flatten();

        let declared_in = obj
            .get("declarations")
            .and_then(|d| d.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(NixOption {
            name: name.to_string(),
            description,
            option_type,
            default,
            example,
            declared: true,
            declared_in,
        })
    }
}

// ---------------------------------------------------------------------------
// Type parsing
// ---------------------------------------------------------------------------

fn parse_type(val: &Value) -> OptionType {
    let name = val
        .get("name")
        .or_else(|| val.get("_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    match name {
        "bool" => OptionType::Bool,
        "int" | "positiveInt" | "unsignedInt" => OptionType::Int {
            min: None,
            max: None,
        },
        "float" | "number" => OptionType::Float,
        "str" | "singleLineStr" | "separatedString" => OptionType::Str,
        "path" => OptionType::Path,
        "package" => OptionType::Package,
        "enum" => {
            let values = val
                .get("values")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            OptionType::Enum { values }
        }
        "listOf" | "coercedTo" => {
            let elem = val
                .get("elemType")
                .map(|t| parse_type(t))
                .unwrap_or(OptionType::Str);
            OptionType::List {
                element: Box::new(elem),
            }
        }
        "attrsOf" | "lazyAttrsOf" => {
            let elem = val
                .get("elemType")
                .map(|t| parse_type(t))
                .unwrap_or(OptionType::Str);
            OptionType::Attrs {
                element: Box::new(elem),
            }
        }
        "nullOr" => {
            let inner = val
                .get("elemType")
                .map(|t| parse_type(t))
                .unwrap_or(OptionType::Str);
            OptionType::Nullable {
                inner: Box::new(inner),
            }
        }
        other => OptionType::Unknown {
            type_str: other.to_string(),
        },
    }
}

fn json_to_value(val: &Value) -> Option<OptionValue> {
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
        Value::Array(arr) => {
            let items: Vec<OptionValue> = arr.iter().filter_map(json_to_value).collect();
            Some(OptionValue::List(items))
        }
        Value::Object(obj) => {
            let mut map = IndexMap::new();
            for (k, v) in obj {
                if let Some(val) = json_to_value(v) {
                    map.insert(k.clone(), val);
                }
            }
            Some(OptionValue::Attrs(map))
        }
    }
}

// ---------------------------------------------------------------------------
// Demo / offline stub
// ---------------------------------------------------------------------------

/// Returns a small hard-coded option set for use when `nix` is not installed.
/// Covers the most common NixOS system options so the TUI is usable for
/// development and demos without a live Nix installation.
pub fn demo_options() -> Vec<NixOption> {
    use OptionType::*;
    use OptionValue::*;

    vec![
        // boot
        NixOption {
            name: "boot.loader.grub.enable".into(),
            description: "Whether to enable the GNU GRUB boot loader.".into(),
            option_type: Bool,
            default: Some(Bool(true)),
            example: Some(Bool(true)),
            declared: true,
            declared_in: Some("nixos/modules/system/boot/loader/grub/grub.nix".into()),
        },
        NixOption {
            name: "boot.loader.grub.device".into(),
            description: "The device on which the GRUB boot loader will be installed. The special value nodev means that a GRUB boot menu will be generated, but GRUB itself will not actually be installed.".into(),
            option_type: Str,
            default: Some(Str("nodev".into())),
            example: Some(Str("/dev/sda".into())),
            declared: true,
            declared_in: Some("nixos/modules/system/boot/loader/grub/grub.nix".into()),
        },
        NixOption {
            name: "boot.loader.grub.efiSupport".into(),
            description: "Whether GRUB should be built with EFI support. EFI support is only available for GRUB v2.".into(),
            option_type: Bool,
            default: Some(Bool(false)),
            example: Some(Bool(true)),
            declared: true,
            declared_in: Some("nixos/modules/system/boot/loader/grub/grub.nix".into()),
        },
        NixOption {
            name: "boot.loader.systemd-boot.enable".into(),
            description: "Whether to enable the systemd-boot (formerly gummiboot) EFI boot manager.".into(),
            option_type: Bool,
            default: Some(Bool(false)),
            example: Some(Bool(true)),
            declared: true,
            declared_in: Some("nixos/modules/system/boot/loader/systemd-boot/systemd-boot.nix".into()),
        },
        NixOption {
            name: "boot.kernelPackages".into(),
            description: "This option allows you to override the Linux kernel used by NixOS.".into(),
            option_type: Package,
            default: Some(Str("pkgs.linuxPackages".into())),
            example: Some(Str("pkgs.linuxPackages_latest".into())),
            declared: true,
            declared_in: None,
        },
        // networking
        NixOption {
            name: "networking.hostName".into(),
            description: "The name of the machine. Leave it empty if you want to obtain it from a DHCP server (not recommended).".into(),
            option_type: Str,
            default: Some(Str("nixos".into())),
            example: Some(Str("myhostname".into())),
            declared: true,
            declared_in: Some("nixos/modules/config/networking.nix".into()),
        },
        NixOption {
            name: "networking.networkmanager.enable".into(),
            description: "Whether to use NetworkManager to obtain an IP address and other configuration for all network interfaces that are not manually configured.".into(),
            option_type: Bool,
            default: Some(Bool(false)),
            example: Some(Bool(true)),
            declared: true,
            declared_in: None,
        },
        NixOption {
            name: "networking.firewall.enable".into(),
            description: "Whether to enable the firewall. This is a simple stateful firewall that blocks connection attempts to unauthorised TCP or UDP ports on this machine.".into(),
            option_type: Bool,
            default: Some(Bool(true)),
            example: Some(Bool(true)),
            declared: true,
            declared_in: None,
        },
        NixOption {
            name: "networking.firewall.allowedTCPPorts".into(),
            description: "List of TCP ports on which incoming connections are accepted.".into(),
            option_type: List { element: Box::new(Int { min: Some(1), max: Some(65535) }) },
            default: Some(List(vec![])),
            example: Some(List(vec![Int(22), Int(80), Int(443)])),
            declared: true,
            declared_in: None,
        },
        // services
        NixOption {
            name: "services.openssh.enable".into(),
            description: "Whether to enable the OpenSSH secure shell daemon, which allows secure remote logins.".into(),
            option_type: Bool,
            default: Some(Bool(false)),
            example: Some(Bool(true)),
            declared: true,
            declared_in: Some("nixos/modules/services/networking/ssh/sshd.nix".into()),
        },
        NixOption {
            name: "services.openssh.permitRootLogin".into(),
            description: "Whether the root user can login using ssh. Valid values are yes, without-password, forced-commands-only, prohibit-password, or no.".into(),
            option_type: Enum {
                values: vec![
                    "yes".into(), "without-password".into(),
                    "forced-commands-only".into(), "prohibit-password".into(), "no".into(),
                ],
            },
            default: Some(Str("prohibit-password".into())),
            example: Some(Str("no".into())),
            declared: true,
            declared_in: Some("nixos/modules/services/networking/ssh/sshd.nix".into()),
        },
        NixOption {
            name: "services.openssh.ports".into(),
            description: "Specifies on which ports the SSH daemon listens.".into(),
            option_type: List { element: Box::new(Int { min: Some(1), max: Some(65535) }) },
            default: Some(List(vec![Int(22)])),
            example: None,
            declared: true,
            declared_in: None,
        },
        NixOption {
            name: "services.xserver.enable".into(),
            description: "Whether to enable the X server.".into(),
            option_type: Bool,
            default: Some(Bool(false)),
            example: Some(Bool(true)),
            declared: true,
            declared_in: None,
        },
        NixOption {
            name: "services.xserver.displayManager.gdm.enable".into(),
            description: "Whether to enable GDM, the GNOME Display Manager.".into(),
            option_type: Bool,
            default: Some(Bool(false)),
            example: Some(Bool(true)),
            declared: true,
            declared_in: None,
        },
        NixOption {
            name: "services.xserver.desktopManager.gnome.enable".into(),
            description: "Enable the GNOME desktop environment.".into(),
            option_type: Bool,
            default: Some(Bool(false)),
            example: Some(Bool(true)),
            declared: true,
            declared_in: None,
        },
        // users
        NixOption {
            name: "users.mutableUsers".into(),
            description: "If set to true, you are free to add new users and groups to the system with the useradd and groupadd commands. On system activation, the existing contents of the /etc/passwd and /etc/group files will be merged with the contents generated from the users.users and users.groups options.".into(),
            option_type: Bool,
            default: Some(Bool(true)),
            example: None,
            declared: true,
            declared_in: None,
        },
        // environment
        NixOption {
            name: "environment.systemPackages".into(),
            description: "The set of packages that appear in /run/current-system/sw. These packages are automatically available to all users, and are automatically updated every time you rebuild the system configuration.".into(),
            option_type: List { element: Box::new(Package) },
            default: Some(List(vec![])),
            example: Some(List(vec![
                Str("pkgs.wget".into()),
                Str("pkgs.vim".into()),
            ])),
            declared: true,
            declared_in: None,
        },
        // time
        NixOption {
            name: "time.timeZone".into(),
            description: "The time zone used when displaying times and dates. See https://en.wikipedia.org/wiki/List_of_tz_database_time_zones for a comprehensive list of possible values for this setting.".into(),
            option_type: Nullable { inner: Box::new(Str) },
            default: Some(Null),
            example: Some(Str("America/New_York".into())),
            declared: true,
            declared_in: None,
        },
        // i18n
        NixOption {
            name: "i18n.defaultLocale".into(),
            description: "The default locale. It determines the language for program messages, the format for dates and currency, and so on. It should be a string like \"de_DE.UTF-8\".".into(),
            option_type: Str,
            default: Some(Str("en_US.UTF-8".into())),
            example: Some(Str("nl_NL.UTF-8".into())),
            declared: true,
            declared_in: None,
        },
        // nix
        NixOption {
            name: "nix.settings.experimental-features".into(),
            description: "A list of experimental features to enable in Nix.".into(),
            option_type: List { element: Box::new(Str) },
            default: Some(List(vec![])),
            example: Some(List(vec![Str("nix-command".into()), Str("flakes".into())])),
            declared: true,
            declared_in: None,
        },
        NixOption {
            name: "nix.gc.automatic".into(),
            description: "Automatically run the Nix garbage collector at a specific time.".into(),
            option_type: Bool,
            default: Some(Bool(false)),
            example: Some(Bool(true)),
            declared: true,
            declared_in: None,
        },
        NixOption {
            name: "nix.gc.dates".into(),
            description: "Specification (in the format described by systemd.time(7)) of the time at which the garbage collector will run. The default is weekly.".into(),
            option_type: Str,
            default: Some(Str("weekly".into())),
            example: Some(Str("03:15".into())),
            declared: true,
            declared_in: None,
        },
    ]
}
