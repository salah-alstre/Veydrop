//! Error type shared by the whole engine. Every variant maps to a stable,
//! machine-readable `code()` that the UIs translate (`errors.<code>`).

use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("too many requests")]
    RateLimited,
    #[error("the pairing code has expired")]
    PairingExpired,
    #[error("the pairing code was already used")]
    PairingUsed,
    #[error("the sharing server is not running")]
    ServerNotRunning,
    #[error("no usable network interface")]
    NoNetwork,
    #[error("not enough free disk space")]
    InsufficientSpace,
    #[error("file is larger than the configured limit")]
    TooLarge,
    #[error("checksum mismatch")]
    HashMismatch,
    #[error("transfer was cancelled")]
    Cancelled,
    /// Upload chunk did not start where the server expects it to (resume point).
    #[error("offset mismatch")]
    OffsetMismatch(u64),
    #[error("the selection is too large for a ZIP archive")]
    ZipTooLarge,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Error::Io(e) => match e.kind() {
                std::io::ErrorKind::PermissionDenied => "permission_denied",
                std::io::ErrorKind::NotFound => "path_not_found",
                std::io::ErrorKind::AddrInUse => "port_in_use",
                _ => "io",
            },
            Error::Invalid(_) => "invalid",
            Error::NotFound => "not_found",
            Error::Conflict(_) => "conflict",
            Error::Unauthorized => "unauthorized",
            Error::Forbidden(_) => "forbidden",
            Error::RateLimited => "rate_limited",
            Error::PairingExpired => "pairing_expired",
            Error::PairingUsed => "pairing_used",
            Error::ServerNotRunning => "server_not_running",
            Error::NoNetwork => "no_network",
            Error::InsufficientSpace => "insufficient_space",
            Error::TooLarge => "too_large",
            Error::HashMismatch => "hash_mismatch",
            Error::Cancelled => "cancelled",
            Error::OffsetMismatch(_) => "offset_mismatch",
            Error::ZipTooLarge => "zip_too_large",
            Error::Other(_) => "other",
        }
    }

    pub fn invalid(msg: impl fmt::Display) -> Self {
        Error::Invalid(msg.to_string())
    }
}

impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Error", 2)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}
