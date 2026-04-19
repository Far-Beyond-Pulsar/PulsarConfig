use std::fmt;

use crate::error::ConfigError;

/// An sRGB color value with 8 bits per channel.
///
/// Conversion helpers to linear float space are provided for renderer interop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// Alpha channel — 255 is fully opaque, 0 is fully transparent.
    pub a: u8,
}

impl Color {
    /// Construct from individual RGBA components.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Construct an opaque color from RGB components.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const WHITE: Color = Color::rgba(255, 255, 255, 255);
    pub const BLACK: Color = Color::rgba(0, 0, 0, 255);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    /// Construct from a packed `0xRRGGBBAA` hex value.
    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 24) & 0xFF) as u8,
            g: ((hex >> 16) & 0xFF) as u8,
            b: ((hex >> 8) & 0xFF) as u8,
            a: (hex & 0xFF) as u8,
        }
    }

    /// Convert to a packed `0xRRGGBBAA` hex value.
    pub const fn to_hex(self) -> u32 {
        ((self.r as u32) << 24)
            | ((self.g as u32) << 16)
            | ((self.b as u32) << 8)
            | (self.a as u32)
    }

    /// Convert the RGB channels from sRGB to linear float, returning `[r, g, b, a]`.
    ///
    /// Useful when passing colors directly to a GPU renderer.
    pub fn to_linear_f32(self) -> [f32; 4] {
        fn to_linear(c: u8) -> f32 {
            let c = c as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        [
            to_linear(self.r),
            to_linear(self.g),
            to_linear(self.b),
            self.a as f32 / 255.0,
        ]
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rgba({}, {}, {}, {})", self.r, self.g, self.b, self.a)
    }
}

// ─── ConfigValue ─────────────────────────────────────────────────────────────

/// A dynamically typed configuration value.
///
/// All variants are cheaply cloneable. Typed accessors (e.g. [`ConfigValue::as_bool`])
/// return a [`ConfigError::TypeMismatch`] instead of panicking.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Color(Color),
    /// An ordered list of values. Elements may be of mixed types.
    Array(Vec<ConfigValue>),
}

impl ConfigValue {
    /// The name of this variant as a `&'static str`, used in error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::Color(_) => "color",
            Self::Array(_) => "array",
        }
    }

    /// Extract a `bool`, or return [`ConfigError::TypeMismatch`].
    pub fn as_bool(&self) -> Result<bool, ConfigError> {
        if let Self::Bool(v) = self {
            Ok(*v)
        } else {
            Err(ConfigError::TypeMismatch {
                expected: "bool",
                got: self.type_name(),
            })
        }
    }

    /// Extract an `i64`, or return [`ConfigError::TypeMismatch`].
    pub fn as_int(&self) -> Result<i64, ConfigError> {
        if let Self::Int(v) = self {
            Ok(*v)
        } else {
            Err(ConfigError::TypeMismatch {
                expected: "int",
                got: self.type_name(),
            })
        }
    }

    /// Extract an `f64`. Integer values are implicitly widened for convenience.
    pub fn as_float(&self) -> Result<f64, ConfigError> {
        match self {
            Self::Float(v) => Ok(*v),
            Self::Int(v) => Ok(*v as f64),
            _ => Err(ConfigError::TypeMismatch {
                expected: "float",
                got: self.type_name(),
            }),
        }
    }

    /// Extract a `&str`, or return [`ConfigError::TypeMismatch`].
    pub fn as_str(&self) -> Result<&str, ConfigError> {
        if let Self::String(v) = self {
            Ok(v.as_str())
        } else {
            Err(ConfigError::TypeMismatch {
                expected: "string",
                got: self.type_name(),
            })
        }
    }

    /// Extract a [`Color`], or return [`ConfigError::TypeMismatch`].
    pub fn as_color(&self) -> Result<Color, ConfigError> {
        if let Self::Color(v) = self {
            Ok(*v)
        } else {
            Err(ConfigError::TypeMismatch {
                expected: "color",
                got: self.type_name(),
            })
        }
    }

    /// Extract a slice of values, or return [`ConfigError::TypeMismatch`].
    pub fn as_array(&self) -> Result<&[ConfigValue], ConfigError> {
        if let Self::Array(v) = self {
            Ok(v.as_slice())
        } else {
            Err(ConfigError::TypeMismatch {
                expected: "array",
                got: self.type_name(),
            })
        }
    }
}

impl fmt::Display for ConfigValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{v}"),
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::String(v) => write!(f, "{v}"),
            Self::Color(v) => write!(f, "{v}"),
            Self::Array(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
        }
    }
}

// ─── From impls ──────────────────────────────────────────────────────────────

impl From<bool> for ConfigValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<i32> for ConfigValue {
    fn from(v: i32) -> Self {
        Self::Int(v as i64)
    }
}
impl From<i64> for ConfigValue {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}
impl From<u32> for ConfigValue {
    fn from(v: u32) -> Self {
        Self::Int(v as i64)
    }
}
impl From<f32> for ConfigValue {
    fn from(v: f32) -> Self {
        Self::Float(v as f64)
    }
}
impl From<f64> for ConfigValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}
impl From<String> for ConfigValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}
impl From<&str> for ConfigValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_owned())
    }
}
impl From<Color> for ConfigValue {
    fn from(v: Color) -> Self {
        Self::Color(v)
    }
}
impl From<Vec<ConfigValue>> for ConfigValue {
    fn from(v: Vec<ConfigValue>) -> Self {
        Self::Array(v)
    }
}
