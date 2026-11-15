//! Configurable user preferences (FEATURES.md X4).
//!
//! Collects the default preferences for axis transforms, automatic range
//! rules, gates, tables, layout defaults and command-line behaviour. Preferences
//! are persisted as JSON and consulted by the GUI / CLI before falling back to
//! built-in defaults; explicit per-workspace settings always take precedence
//! over the global preferences.

use crate::transform::AxisScale;
use serde::{Deserialize, Serialize};

/// Preferences for axis transforms and automatic ranges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformPreferences {
    /// Default X axis scale used when a plot has none configured.
    #[serde(default)]
    pub default_x_scale: AxisScale,
    /// Default Y axis scale used when a plot has none configured.
    #[serde(default)]
    pub default_y_scale: AxisScale,
}

impl Default for TransformPreferences {
    fn default() -> Self {
        Self {
            default_x_scale: AxisScale::Linear,
            default_y_scale: AxisScale::Linear,
        }
    }
}

/// Preferences for automatic axis-range rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AutorangePreferences {
    /// Whether automatic range rules are enabled by default.
    #[serde(default)]
    pub enabled: bool,
}

/// Preferences for gate defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatePreferences {
    /// Default gate line width.
    #[serde(default)]
    pub default_line_width: f32,
}

impl Default for GatePreferences {
    fn default() -> Self {
        Self {
            default_line_width: 1.5,
        }
    }
}

/// Preferences for table display defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TablePreferences {
    /// Default page size for paginated table output.
    #[serde(default)]
    pub default_page_size: usize,
}

impl Default for TablePreferences {
    fn default() -> Self {
        Self {
            default_page_size: 100,
        }
    }
}

/// Preferences for layout defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutPreferences {
    /// Default layout font size.
    #[serde(default)]
    pub default_font_size: f32,
}

impl Default for LayoutPreferences {
    fn default() -> Self {
        Self {
            default_font_size: 13.0,
        }
    }
}

/// Command-line behaviour preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CommandLinePreferences {
    /// Verbose logging.
    #[serde(default)]
    pub verbose: bool,
}

/// Global user preferences (X4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Preferences {
    #[serde(default)]
    pub transforms: TransformPreferences,
    #[serde(default)]
    pub autorange: AutorangePreferences,
    #[serde(default)]
    pub gates: GatePreferences,
    #[serde(default)]
    pub tables: TablePreferences,
    #[serde(default)]
    pub layout: LayoutPreferences,
    #[serde(default)]
    pub command_line: CommandLinePreferences,
}

impl Preferences {
    /// A fresh set of default preferences.
    pub fn new() -> Self {
        Self::default()
    }

    /// Effective default X axis scale (preference, else built-in linear).
    pub fn default_x_scale(&self) -> AxisScale {
        self.transforms.default_x_scale.clone()
    }

    /// Effective default Y axis scale (preference, else built-in linear).
    pub fn default_y_scale(&self) -> AxisScale {
        self.transforms.default_y_scale.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_default_to_builtin_scales_and_flag_defaults() {
        let prefs = Preferences::new();
        assert_eq!(prefs.default_x_scale(), AxisScale::Linear);
        assert_eq!(prefs.default_y_scale(), AxisScale::Linear);
        assert!(!prefs.autorange.enabled);
        assert_eq!(prefs.gates.default_line_width, 1.5);
        assert_eq!(prefs.tables.default_page_size, 100);
        assert_eq!(prefs.layout.default_font_size, 13.0);
        assert!(!prefs.command_line.verbose);
    }

    #[test]
    fn preferences_round_trip_through_serde() {
        let mut prefs = Preferences::new();
        prefs.transforms.default_x_scale = AxisScale::Log;
        prefs.transforms.default_y_scale = AxisScale::Arcsinh { cofactor: 150.0 };
        prefs.autorange.enabled = true;
        prefs.gates.default_line_width = 2.5;
        prefs.tables.default_page_size = 25;
        prefs.layout.default_font_size = 9.0;
        prefs.command_line.verbose = true;
        let json = serde_json::to_string(&prefs).unwrap();
        let back: Preferences = serde_json::from_str(&json).unwrap();
        assert_eq!(back, prefs);
    }

    #[test]
    fn preferences_missing_sections_fall_back_to_defaults() {
        // An old preferences file without any section must still load.
        let back: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(back, Preferences::new());
    }
}
