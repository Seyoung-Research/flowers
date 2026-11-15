//! Plugin system for workspace, population and transform plugins (X2).
//!
//! A [`Plugin`] advertises a stable name, version and capability. [`PluginHost`]
//! keeps a registry of plugins and resolves them by name; the host is the
//! extension point used by the scripting console, batch runner and future GUI
//! plugin dialogs. Capabilities are intentionally coarse: the host returns the
//! capability tag so callers can dispatch to the right integration point
//! without leaking plugin internals into the core.

use serde::{Deserialize, Serialize};

/// What kind of behaviour a plugin provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginCapability {
    /// Workspace-level plugins (sample grouping, annotation, ...).
    Workspace,
    /// Population-level plugins (automatic gating strategies, ...).
    Population,
    /// Transform plugins (cytometer-specific axis transforms).
    Transform,
}

/// A registered plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct Plugin {
    pub name: String,
    pub version: String,
    pub capability: PluginCapability,
}

/// Registry of plugins by name (X2).
#[derive(Clone, Default)]
pub struct PluginHost {
    plugins: Vec<Plugin>,
}

impl PluginHost {
    /// A fresh, empty host.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a plugin; a plugin with the same name replaces the previous one.
    /// Returns `true` when a plugin was added, `false` when the name was unknown
    /// and the plugin should be treated as new.
    pub fn register(&mut self, plugin: Plugin) -> bool {
        if self.plugins.iter().any(|p| p.name == plugin.name) {
            if let Some(existing) = self.plugins.iter_mut().find(|p| p.name == plugin.name) {
                *existing = plugin;
            }
            return true;
        }
        self.plugins.push(plugin);
        true
    }

    /// All registered plugins, in registration order.
    pub fn list(&self) -> &[Plugin] {
        &self.plugins
    }

    /// Look up a plugin by name.
    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.name == name)
    }

    /// Resolve the capability of a named plugin.
    pub fn capability(&self, name: &str) -> Option<PluginCapability> {
        self.get(name).map(|p| p.capability)
    }

    /// Remove every plugin (reset).
    pub fn clear(&mut self) {
        self.plugins.clear();
    }

    /// Number of registered plugins.
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

impl std::fmt::Debug for PluginHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHost")
            .field(
                "plugins",
                &self
                    .plugins
                    .iter()
                    .map(|p| (p.name.clone(), p.version.clone()))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(name: &str, capability: PluginCapability) -> Plugin {
        Plugin {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            capability,
        }
    }

    #[test]
    fn plugin_host_starts_empty_and_registers() {
        let mut host = PluginHost::new();
        assert!(host.is_empty());
        assert_eq!(host.len(), 0);
        assert!(host.register(plugin("flowjo-like", PluginCapability::Transform)));
        assert_eq!(host.len(), 1);
        assert!(host.get("flowjo-like").is_some());
        assert_eq!(
            host.capability("flowjo-like"),
            Some(PluginCapability::Transform)
        );
        assert_eq!(host.capability("missing"), None);
    }

    #[test]
    fn plugin_host_replaces_same_name() {
        let mut host = PluginHost::new();
        host.register(plugin("abc", PluginCapability::Population));
        host.register(plugin("abc", PluginCapability::Workspace));
        assert_eq!(host.len(), 1);
        assert_eq!(host.capability("abc"), Some(PluginCapability::Workspace));
    }

    #[test]
    fn plugin_host_clear_resets() {
        let mut host = PluginHost::new();
        host.register(plugin("abc", PluginCapability::Transform));
        assert_eq!(host.len(), 1);
        host.clear();
        assert!(host.is_empty());
        assert_eq!(host.get("abc"), None);
    }
}
