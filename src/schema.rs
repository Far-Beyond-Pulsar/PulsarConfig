use std::collections::HashMap;
use std::sync::Arc;

use crate::value::ConfigValue;

// ─── UI field metadata ────────────────────────────────────────────────────────

/// An option in a [`FieldType::Dropdown`] control.
#[derive(Debug, Clone, PartialEq)]
pub struct DropdownOption {
    pub label: String,
    pub value: String,
}

impl DropdownOption {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self { label: label.into(), value: value.into() }
    }
    /// Shorthand when the label and the value are the same string.
    pub fn same(s: impl Into<String>) -> Self {
        let s = s.into();
        Self { label: s.clone(), value: s }
    }
}

/// Hint to a settings UI about which widget to render for a key.
///
/// This is **optional metadata** — the config system does not use it
/// internally. Pass it via [`SchemaEntry::field_type`] so that settings-screen
/// code can render the appropriate control without needing a parallel registry.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    /// A boolean toggle / checkbox.
    Checkbox,
    /// A numeric text input with optional bounds.
    NumberInput {
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    },
    /// A free-text input field.
    TextInput {
        placeholder: Option<String>,
        multiline: bool,
    },
    /// A dropdown / select widget backed by an explicit option list.
    Dropdown { options: Vec<DropdownOption> },
    /// A draggable slider with explicit bounds.
    Slider { min: f64, max: f64, step: f64 },
    /// An RGBA color picker.
    ColorPicker,
    /// A file/directory path chooser.
    PathSelector { directory: bool },
}

/// A validation function supplied by the caller.
pub type ValidatorFn = Arc<dyn Fn(&ConfigValue) -> Result<(), String> + Send + Sync>;

/// A constraint applied to a [`ConfigValue`] before it is stored.
///
/// Multiple validators may be attached to a single setting; all must pass.
pub enum Validator {
    /// The integer value must lie within `[min, max]` (inclusive).
    IntRange { min: i64, max: i64 },

    /// The float value must lie within `[min, max]` (inclusive).
    FloatRange { min: f64, max: f64 },

    /// The string's byte-length must not exceed `max`.
    StringMaxLength(usize),

    /// The string must exactly match one of the listed options.
    StringOneOf(Vec<String>),

    /// A caller-supplied validation closure.
    Custom(ValidatorFn),
}

impl Validator {
    pub fn int_range(min: i64, max: i64) -> Self {
        Self::IntRange { min, max }
    }

    pub fn float_range(min: f64, max: f64) -> Self {
        Self::FloatRange { min, max }
    }

    pub fn string_max_length(max: usize) -> Self {
        Self::StringMaxLength(max)
    }

    /// Accept only the listed string values (case-sensitive).
    pub fn string_one_of(
        options: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self::StringOneOf(options.into_iter().map(Into::into).collect())
    }

    /// Supply an arbitrary validation closure.
    ///
    /// Return `Ok(())` to accept the value or `Err(reason)` to reject it.
    pub fn custom(
        f: impl Fn(&ConfigValue) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self::Custom(Arc::new(f))
    }

    /// Run the validator against `value`. Returns an error string on failure.
    pub(crate) fn validate(&self, value: &ConfigValue) -> Result<(), String> {
        match self {
            Self::IntRange { min, max } => {
                let v = value.as_int().map_err(|e| e.to_string())?;
                if v >= *min && v <= *max {
                    Ok(())
                } else {
                    Err(format!("value {v} is out of range [{min}, {max}]"))
                }
            }
            Self::FloatRange { min, max } => {
                let v = value.as_float().map_err(|e| e.to_string())?;
                if v >= *min && v <= *max {
                    Ok(())
                } else {
                    Err(format!("value {v} is out of range [{min}, {max}]"))
                }
            }
            Self::StringMaxLength(max) => {
                let v = value.as_str().map_err(|e| e.to_string())?;
                if v.len() <= *max {
                    Ok(())
                } else {
                    Err(format!(
                        "string length {} exceeds maximum {max}",
                        v.len()
                    ))
                }
            }
            Self::StringOneOf(options) => {
                let v = value.as_str().map_err(|e| e.to_string())?;
                if options.iter().any(|o| o == v) {
                    Ok(())
                } else {
                    Err(format!(
                        "'{v}' is not one of the allowed values: {:?}",
                        options
                    ))
                }
            }
            Self::Custom(f) => f(value),
        }
    }
}

// ─── SchemaEntry ─────────────────────────────────────────────────────────────

/// Declares a single configuration key: its description, default value,
/// validation rules, and optional metadata tags.
///
/// Built with a fluent API:
///
/// ```rust
/// use pulsar_config::{SchemaEntry, Validator};
///
/// let entry = SchemaEntry::new("Maximum shadow draw distance", 500.0_f64)
///     .validator(Validator::float_range(0.0, 10_000.0))
///     .tag("performance")
///     .tag("rendering");
/// ```
pub struct SchemaEntry {
    /// Human-readable description shown in editors and search results.
    pub description: String,
    /// Short human-readable label (used in settings-screen rows).
    /// Falls back to the key name when absent.
    pub label: Option<String>,
    /// UI grouping page (e.g. `"Appearance"`, `"Editor"`).
    /// Only meaningful to settings-screen code; ignored by the config engine.
    pub page: Option<String>,
    /// The value used when no override has been set.
    pub default: ConfigValue,
    /// Validators run (in order) on every attempted write.
    pub validators: Vec<Validator>,
    /// Free-form tags used for filtering and search (e.g. `"performance"`, `"rendering"`).
    pub tags: Vec<String>,
    /// If `true`, writes via [`OwnerHandle::set`](crate::OwnerHandle::set) are rejected.
    pub read_only: bool,
    /// Optional widget hint for settings-screen UIs.
    pub field_type: Option<FieldType>,
}

impl SchemaEntry {
    /// Create a new entry with the given description and default value.
    ///
    /// Any type that implements `Into<ConfigValue>` is accepted as the default
    /// (e.g. `true`, `42_i64`, `"medium"`, [`Color`](crate::Color)).
    pub fn new(
        description: impl Into<String>,
        default: impl Into<ConfigValue>,
    ) -> Self {
        Self {
            description: description.into(),
            label: None,
            page: None,
            default: default.into(),
            validators: Vec::new(),
            tags: Vec::new(),
            read_only: false,
            field_type: None,
        }
    }

    /// Attach a validator. Multiple validators may be chained.
    pub fn validator(mut self, v: Validator) -> Self {
        self.validators.push(v);
        self
    }

    /// Set a short human-readable label (used as the row heading in settings UIs).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Assign this setting to a UI page (e.g. `"Appearance"`, `"Editor"`).
    pub fn page(mut self, page: impl Into<String>) -> Self {
        self.page = Some(page.into());
        self
    }

    /// Provide a widget hint to the settings-screen renderer.
    pub fn field_type(mut self, ft: FieldType) -> Self {
        self.field_type = Some(ft);
        self
    }

    /// Add a single metadata tag.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Add multiple metadata tags at once.
    pub fn tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tags.extend(tags.into_iter().map(Into::into));
        self
    }

    /// Mark this setting as read-only at runtime.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Run all attached validators against `value`.
    pub(crate) fn validate(&self, value: &ConfigValue) -> Result<(), String> {
        for v in &self.validators {
            v.validate(value)?;
        }
        Ok(())
    }
}

// ─── NamespaceSchema ─────────────────────────────────────────────────────────

/// Declares all configuration keys belonging to one plugin or subsystem.
///
/// Pass to [`ConfigManager::register_namespace`] to activate the namespace and
/// receive a [`NamespaceHandle`](crate::NamespaceHandle).
///
/// ```rust
/// use pulsar_config::{NamespaceSchema, SchemaEntry, Validator};
///
/// let schema = NamespaceSchema::new("Shadow Renderer", "Shadow rendering settings")
///     .setting("enabled", SchemaEntry::new("Enable shadow rendering", true))
///     .setting(
///         "quality",
///         SchemaEntry::new("Quality preset", "high")
///             .validator(Validator::string_one_of(["low", "medium", "high", "ultra"])),
///     );
/// ```
pub struct NamespaceSchema {
    /// Display name shown in editors and debug output.
    pub display_name: String,
    /// Short description of what this namespace configures.
    pub description: String,
    pub(crate) entries: HashMap<String, SchemaEntry>,
}

impl NamespaceSchema {
    pub fn new(
        display_name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            description: description.into(),
            entries: HashMap::new(),
        }
    }

    /// Declare a setting. `key` must be unique within this namespace.
    pub fn setting(mut self, key: impl Into<String>, entry: SchemaEntry) -> Self {
        self.entries.insert(key.into(), entry);
        self
    }
}
