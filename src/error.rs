use std::fmt;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Format an owner path as a human-readable string (`"seg1/seg2/seg3"`).
pub(crate) fn fmt_owner(owner: &[String]) -> String {
    owner.join("/")
}

// ─── ConfigError ─────────────────────────────────────────────────────────────

/// All errors that can arise from configuration operations.
///
/// Every variant that refers to a specific location in the hierarchy carries
/// both the top-level `namespace` and the full `owner` path so that error
/// messages are self-contained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// An owner at `(namespace, owner)` is already registered.
    OwnerAlreadyRegistered {
        namespace: String,
        /// The full owner path segments, e.g. `["subsystem", "physics"]`.
        owner: Vec<String>,
    },

    /// No owner at `(namespace, owner)` is registered.
    OwnerNotFound {
        namespace: String,
        owner: Vec<String>,
    },

    /// The key is not declared in the owner's schema.
    UnknownKey {
        namespace: String,
        owner: Vec<String>,
        key: String,
    },

    /// The value failed one or more validators.
    ValidationFailed {
        namespace: String,
        owner: Vec<String>,
        key: String,
        reason: String,
    },

    /// The setting is declared read-only in the schema.
    ReadOnly {
        namespace: String,
        owner: Vec<String>,
        key: String,
    },

    /// Typed accessor was called on the wrong variant.
    TypeMismatch {
        expected: &'static str,
        got: &'static str,
    },

    /// A namespace name or owner path segment contains the null byte (`\0`),
    /// which is reserved as an internal separator.
    InvalidIdentifier(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerAlreadyRegistered { namespace, owner } => write!(
                f,
                "owner '{namespace}/{owner}' is already registered",
                owner = fmt_owner(owner)
            ),
            Self::OwnerNotFound { namespace, owner } => write!(
                f,
                "owner '{namespace}/{owner}' not found",
                owner = fmt_owner(owner)
            ),
            Self::UnknownKey { namespace, owner, key } => write!(
                f,
                "key '{key}' is not defined for owner '{namespace}/{owner}'",
                owner = fmt_owner(owner)
            ),
            Self::ValidationFailed { namespace, owner, key, reason } => write!(
                f,
                "validation failed for '{namespace}/{owner}::{key}': {reason}",
                owner = fmt_owner(owner)
            ),
            Self::ReadOnly { namespace, owner, key } => write!(
                f,
                "setting '{namespace}/{owner}::{key}' is read-only",
                owner = fmt_owner(owner)
            ),
            Self::TypeMismatch { expected, got } => {
                write!(f, "type mismatch: expected {expected}, got {got}")
            }
            Self::InvalidIdentifier(msg) => write!(f, "invalid identifier: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}
