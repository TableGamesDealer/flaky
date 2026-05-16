use indexmap::IndexMap;

use crate::NixOption;

/// Organises the flat list of NixOS options into a tree structure suitable
/// for TUI navigation.  The root categories are the top-level dotted segments
/// (e.g. "boot", "networking", "services", "users").
pub struct SchemaStore {
    /// All options by dotted name, in declaration order.
    pub options: IndexMap<String, NixOption>,
    /// Category → list of option names in that category.
    categories: IndexMap<String, Vec<String>>,
}

impl SchemaStore {
    pub fn from_options(opts: Vec<NixOption>) -> Self {
        let mut options = IndexMap::new();
        let mut categories: IndexMap<String, Vec<String>> = IndexMap::new();

        for opt in opts {
            let cat = top_level_category(&opt.name);
            categories.entry(cat).or_default().push(opt.name.clone());
            options.insert(opt.name.clone(), opt);
        }

        Self {
            options,
            categories,
        }
    }

    /// Sorted list of top-level category names.
    pub fn category_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.categories.keys().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names
    }

    /// Options directly under a given dotted prefix (one level deep).
    pub fn children_of(&self, prefix: &str) -> Vec<&NixOption> {
        let search = if prefix.is_empty() {
            // Root: return all top-level categories as synthetic options.
            return self
                .category_names()
                .into_iter()
                .filter_map(|c| self.options.get(c))
                .collect();
        } else {
            format!("{prefix}.")
        };

        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();

        for name in self.options.keys() {
            if name.starts_with(&search) {
                let remainder = &name[search.len()..];
                // Take only one more segment.
                let next_seg = remainder.split('.').next().unwrap_or(remainder);
                let child_path = format!("{prefix}.{next_seg}");
                if seen.insert(child_path.clone()) {
                    if let Some(opt) = self.options.get(&child_path) {
                        out.push(opt);
                    } else {
                        // Intermediate node — not a leaf option itself.
                        // We synthesize it later if needed; skip for now.
                    }
                }
            }
        }

        out
    }

    /// Fuzzy text search over option names and descriptions.
    /// Returns matches sorted by relevance (simple contains match for now;
    /// nucleo integration added in the search module).
    pub fn search(&self, query: &str) -> Vec<&NixOption> {
        let q = query.to_lowercase();
        let mut results: Vec<(&NixOption, usize)> = self
            .options
            .values()
            .filter_map(|opt| {
                let name_score = if opt.name.to_lowercase().contains(&q) {
                    2
                } else {
                    0
                };
                let desc_score = if opt.description.to_lowercase().contains(&q) {
                    1
                } else {
                    0
                };
                let score = name_score + desc_score;
                if score > 0 { Some((opt, score)) } else { None }
            })
            .collect();

        results.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.name.cmp(&b.0.name)));
        results.into_iter().map(|(o, _)| o).collect()
    }

    /// Total number of known options.
    pub fn len(&self) -> usize {
        self.options.len()
    }

    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
    }
}

fn top_level_category(dotted_name: &str) -> String {
    dotted_name
        .split('.')
        .next()
        .unwrap_or(dotted_name)
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::{OptionType, OptionValue};

    use super::*;

    fn make_opt(name: &str) -> NixOption {
        NixOption {
            name: name.into(),
            description: format!("Option {name}"),
            option_type: OptionType::Bool,
            default: Some(OptionValue::Bool(false)),
            example: None,
            declared: true,
            declared_in: None,
        }
    }

    fn sample_store() -> SchemaStore {
        SchemaStore::from_options(vec![
            make_opt("boot.loader.grub.enable"),
            make_opt("boot.loader.grub.device"),
            make_opt("networking.hostName"),
            make_opt("networking.firewall.enable"),
            make_opt("services.openssh.enable"),
            make_opt("services.openssh.permitRootLogin"),
        ])
    }

    #[test]
    fn categories() {
        let store = sample_store();
        let cats = store.category_names();
        assert!(cats.contains(&"boot"));
        assert!(cats.contains(&"networking"));
        assert!(cats.contains(&"services"));
    }

    #[test]
    fn search_finds_by_name() {
        let store = sample_store();
        let results = store.search("openssh");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_is_case_insensitive() {
        let store = sample_store();
        assert!(!store.search("GRUB").is_empty());
    }
}
