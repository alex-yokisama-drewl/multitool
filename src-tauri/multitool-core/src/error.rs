use serde::{ser::SerializeStruct, Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("file not found: {path}")]
    FileNotFound { path: String },

    #[error("permission denied: {path}")]
    PermissionDenied { path: String },

    #[error("unsupported format: {detail}")]
    UnsupportedFormat { detail: String },

    #[error("processing failed: {detail}")]
    ProcessingFailed { detail: String },

    #[error("password-protected PDF; password entry is not supported in Phase 1")]
    Encrypted,

    #[error("operation cancelled")]
    Cancelled,
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::FileNotFound { .. } => "FileNotFound",
            Self::PermissionDenied { .. } => "PermissionDenied",
            Self::UnsupportedFormat { .. } => "UnsupportedFormat",
            Self::ProcessingFailed { .. } => "ProcessingFailed",
            Self::Encrypted => "Encrypted",
            Self::Cancelled => "Cancelled",
        }
    }
}

// Serialize as `{ kind, message }` so the webview can branch on `kind`
// while still surfacing a human-readable string for toasts.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

pub type AppResult<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_kind_and_message() {
        let err = AppError::FileNotFound {
            path: "/tmp/missing.pdf".into(),
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "FileNotFound");
        assert_eq!(json["message"], "file not found: /tmp/missing.pdf");
    }

    #[test]
    fn cancelled_has_no_payload() {
        let err = AppError::Cancelled;
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "Cancelled");
        assert_eq!(json["message"], "operation cancelled");
    }

    #[test]
    fn encrypted_has_no_payload_and_explains_phase_1_limit() {
        let err = AppError::Encrypted;
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "Encrypted");
        assert_eq!(
            json["message"],
            "password-protected PDF; password entry is not supported in Phase 1"
        );
    }
}
