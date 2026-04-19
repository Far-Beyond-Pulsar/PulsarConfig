use std::fmt;

/// All errors that can arise from configuration operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A namespace with this ID has already been registered.
    NamespaceAlreadyRegistered(String),

    /// No namespace with this ID exists.
    NamespaceNotFound(String),

    /// The key is not declared in the namespace's schema.
    UnknownKey { namespace: String, key: String },

    /// The value failed one or more validators.
    ValidationFailed {
        namespace: String,
        key: String,
        reason: String,
    },

    /// The setting is declared read-only in the schema.
    ReadOnly { namespace: String, key: String },

    /// Typed accessor was called on the wrong variant.
    TypeMismatch {
        expected: &'static str,
        got: &'static str,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NamespaceAlreadyRegistered(id) => {
                write!(f, "namespace '{id}' is already registered")
            }
            Self::NamespaceNotFound(id) => {
                write!(f, "namespace '{id}' not found")
            }
            Self::UnknownKey { namespace, key } => {
                write!(f, "key '{key}' is not defined in namespace '{namespace}'")
            }
            Self::ValidationFailed {
                namespace,
                key,
                reason,
            } => {
                write!(
                    f,
                    "validation failed for '{namespace}::{key}': {reason}"
                )
            }
            Self::ReadOnly { namespace, key } => {
                write!(f, "setting '{namespace}::{key}' is read-only")
            }
            Self::TypeMismatch { expected, got } => {
                write!(f, "type mismatch: expected {expected}, got {got}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}
