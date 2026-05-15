use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// The type of a NixOS option, mapping directly to a TUI widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OptionType {
    /// bool → toggle
    Bool,
    /// int → numeric input
    Int { min: Option<i64>, max: Option<i64> },
    /// float → numeric input
    Float,
    /// string → text input
    Str,
    /// path → text input with path hint
    Path,
    /// package → package picker
    Package,
    /// enum (one-of) → radio/select
    Enum { values: Vec<String> },
    /// listOf T → multi-entry list
    List { element: Box<OptionType> },
    /// attrsOf T → key/value editor
    Attrs { element: Box<OptionType> },
    /// submodule → nested settings page
    Submodule {
        options: IndexMap<String, NixOption>,
    },
    /// nullOr T → toggle to enable, then inner widget
    Nullable { inner: Box<OptionType> },
    /// Anything we can't parse yet — show as raw text
    Unknown { type_str: String },
}

impl OptionType {
    /// Human-readable widget hint shown in the TUI footer.
    pub fn widget_hint(&self) -> &'static str {
        match self {
            OptionType::Bool => "space to toggle",
            OptionType::Int { .. } | OptionType::Float => "type a number",
            OptionType::Str | OptionType::Path => "type a value",
            OptionType::Package => "/ to search packages",
            OptionType::Enum { .. } => "← → to cycle",
            OptionType::List { .. } => "a to add  d to delete",
            OptionType::Attrs { .. } => "a to add  d to delete",
            OptionType::Submodule { .. } => "enter to expand",
            OptionType::Nullable { .. } => "space to enable",
            OptionType::Unknown { .. } => "enter to edit raw",
        }
    }
}

/// A concrete value stored for an option.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptionValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<OptionValue>),
    Attrs(IndexMap<String, OptionValue>),
    Null,
}

impl OptionValue {
    pub fn display(&self) -> String {
        match self {
            OptionValue::Bool(b) => {
                if *b {
                    "enabled".into()
                } else {
                    "disabled".into()
                }
            }
            OptionValue::Int(n) => n.to_string(),
            OptionValue::Float(f) => format!("{f:.2}"),
            OptionValue::Str(s) => format!("\"{s}\""),
            OptionValue::List(v) => format!("[{} items]", v.len()),
            OptionValue::Attrs(m) => format!("{{ {} attrs }}", m.len()),
            OptionValue::Null => "null (disabled)".into(),
        }
    }

    pub fn type_matches(&self, ty: &OptionType) -> bool {
        matches!(
            (self, ty),
            (OptionValue::Bool(_), OptionType::Bool)
                | (OptionValue::Int(_), OptionType::Int { .. })
                | (OptionValue::Float(_), OptionType::Float)
                | (OptionValue::Str(_), OptionType::Str)
                | (OptionValue::Str(_), OptionType::Path)
                | (OptionValue::Str(_), OptionType::Package)
                | (OptionValue::Str(_), OptionType::Enum { .. })
                | (OptionValue::List(_), OptionType::List { .. })
                | (OptionValue::Attrs(_), OptionType::Attrs { .. })
                | (OptionValue::Null, OptionType::Nullable { .. })
        )
    }
}

impl std::fmt::Display for OptionValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}

/// A single NixOS option as extracted from `nix eval`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NixOption {
    /// Dotted path, e.g. "services.openssh.enable"
    pub name: String,
    /// Human-readable description from the NixOS module docs.
    pub description: String,
    /// Type descriptor.
    pub option_type: OptionType,
    /// Default value if set.
    pub default: Option<OptionValue>,
    /// Example value from the docs.
    pub example: Option<OptionValue>,
    /// Whether this option is declared by a NixOS module.
    pub declared: bool,
    /// Nix file that declares this option (for "open source" action).
    pub declared_in: Option<String>,
}

impl NixOption {
    /// Short label for display in the TUI list.
    pub fn short_name(&self) -> &str {
        self.name.rsplit('.').next().unwrap_or(&self.name)
    }

    /// Category breadcrumb: everything but the last segment.
    pub fn category(&self) -> String {
        let parts: Vec<&str> = self.name.splitn(usize::MAX, '.').collect();
        if parts.len() > 1 {
            parts[..parts.len() - 1].join(".")
        } else {
            String::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_display() {
        assert_eq!(OptionValue::Bool(true).display(), "enabled");
        assert_eq!(OptionValue::Bool(false).display(), "disabled");
    }

    #[test]
    fn short_name() {
        let opt = NixOption {
            name: "services.openssh.enable".into(),
            description: "".into(),
            option_type: OptionType::Bool,
            default: None,
            example: None,
            declared: true,
            declared_in: None,
        };
        assert_eq!(opt.short_name(), "enable");
        assert_eq!(opt.category(), "services.openssh");
    }
}
