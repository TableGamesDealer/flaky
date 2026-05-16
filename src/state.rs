use anyhow::{Result, anyhow};
use std::collections::HashMap;

use crate::{NixOption, OptionValue};

/// A single reversible edit.
#[derive(Debug, Clone)]
pub struct Edit {
    pub option_name: String,
    pub before: Option<OptionValue>,
    pub after: Option<OptionValue>,
}

/// Tracks all current values and pending changes for a configuration session.
///
/// Invariant: `current` always reflects the baseline values loaded from disk.
/// `pending` overlays edits the user has made but not yet saved.
/// `undo_stack` lets us walk backward through edits.
#[derive(Debug, Default)]
pub struct ConfigState {
    /// Values as loaded from flake.nix / the running system.
    current: HashMap<String, OptionValue>,
    /// User's unsaved edits (option name → new value, or None = "reset to default").
    pending: HashMap<String, Option<OptionValue>>,
    /// Undo history — most recent first.
    undo_stack: Vec<Edit>,
    /// Maximum undo depth.
    max_undo: usize,
}

impl ConfigState {
    pub fn new() -> Self {
        Self {
            max_undo: 100,
            ..Default::default()
        }
    }

    /// Load baseline values (called after parsing flake.nix).
    pub fn load_current(&mut self, values: HashMap<String, OptionValue>) {
        self.current = values;
        self.pending.clear();
        self.undo_stack.clear();
    }

    /// Return the effective value for an option: pending overrides current.
    pub fn get(&self, name: &str) -> Option<&OptionValue> {
        if let Some(pending) = self.pending.get(name) {
            pending.as_ref()
        } else {
            self.current.get(name)
        }
    }

    /// Apply a user edit, recording the previous value for undo.
    pub fn set(&mut self, option: &NixOption, new_value: Option<OptionValue>) -> Result<()> {
        // Type-check if a value is provided.
        if let Some(ref val) = new_value {
            if !val.type_matches(&option.option_type) {
                return Err(anyhow!(
                    "type mismatch: option '{}' expects {:?}, got {:?}",
                    option.name,
                    option.option_type,
                    val
                ));
            }
        }

        let before = self.get(&option.name).cloned();
        let after = new_value.clone();

        self.pending.insert(option.name.clone(), new_value);

        let edit = Edit {
            option_name: option.name.clone(),
            before,
            after,
        };

        if self.undo_stack.len() >= self.max_undo {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(edit);

        Ok(())
    }

    /// Undo the last edit.
    pub fn undo(&mut self) -> Option<String> {
        let edit = self.undo_stack.pop()?;
        match &edit.before {
            Some(prev) => {
                self.pending
                    .insert(edit.option_name.clone(), Some(prev.clone()));
            }
            None => {
                // Was previously unset — remove from pending (fall through to current).
                self.pending.remove(&edit.option_name);
            }
        }
        Some(edit.option_name)
    }

    /// True if there are unsaved changes.
    pub fn is_dirty(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Number of pending changes.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Consume pending changes and return them for the flake writer.
    /// Clears the pending map (caller must handle commit / rollback on write failure).
    pub fn take_pending(&mut self) -> HashMap<String, Option<OptionValue>> {
        let out = self.pending.clone();
        // Merge into current (as if saved).
        for (k, v) in &out {
            match v {
                Some(val) => {
                    self.current.insert(k.clone(), val.clone());
                }
                None => {
                    self.current.remove(k);
                }
            }
        }
        self.pending.clear();
        self.undo_stack.clear();
        out
    }

    /// Discard all pending changes.
    pub fn revert(&mut self) {
        self.pending.clear();
        self.undo_stack.clear();
    }

    /// List all option names that have pending changes.
    pub fn dirty_names(&self) -> Vec<&String> {
        self.pending.keys().collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::OptionType;

    use super::*;

    fn bool_option(name: &str) -> NixOption {
        NixOption {
            name: name.into(),
            description: "".into(),
            option_type: OptionType::Bool,
            default: None,
            example: None,
            declared: true,
            declared_in: None,
        }
    }

    #[test]
    fn set_and_get() {
        let mut state = ConfigState::new();
        let opt = bool_option("services.openssh.enable");
        state.set(&opt, Some(OptionValue::Bool(true))).unwrap();
        assert_eq!(
            state.get("services.openssh.enable"),
            Some(&OptionValue::Bool(true))
        );
        assert!(state.is_dirty());
    }

    #[test]
    fn undo_restores_previous() {
        let mut state = ConfigState::new();
        let opt = bool_option("services.openssh.enable");
        state.load_current({
            let mut m = HashMap::new();
            m.insert("services.openssh.enable".into(), OptionValue::Bool(false));
            m
        });
        state.set(&opt, Some(OptionValue::Bool(true))).unwrap();
        let name = state.undo().unwrap();
        assert_eq!(name, "services.openssh.enable");
        assert_eq!(
            state.get("services.openssh.enable"),
            Some(&OptionValue::Bool(false))
        );
    }

    #[test]
    fn take_pending_clears_dirty() {
        let mut state = ConfigState::new();
        let opt = bool_option("boot.loader.grub.enable");
        state.set(&opt, Some(OptionValue::Bool(true))).unwrap();
        assert!(state.is_dirty());
        let changes = state.take_pending();
        assert!(!state.is_dirty());
        assert!(changes.contains_key("boot.loader.grub.enable"));
    }
}
