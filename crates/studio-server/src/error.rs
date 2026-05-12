//! `RouteError` — uniform HTTP error type for Wave A4 routes.
//!
//! Per CLAUDE.md §4 ("structured JSON over the wire") every fallible
//! handler returns `Result<axum::response::Response, RouteError>`. The
//! `IntoResponse` implementation renders a JSON body of the form:
//!
//! ```json
//! { "error": "<human message>", "code": "<machine code>" }
//! ```
//!
//! Variants are coarse on purpose — the M2 frontend keys off the `code`
//! field, not on a `match`-rich enum. The mapping from
//! [`studio_store::StoreError`] respects `is_not_found()` so 404s are not
//! collapsed into 500s.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Error returned by Wave A4 route handlers.
///
/// Implements [`IntoResponse`] so handlers can return
/// `Result<Response, RouteError>` and let Axum render the JSON body.
#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    /// Requested resource does not exist. Renders as `404` with `code`
    /// set by the caller (`adr_not_found`, `finding_not_found`, ...).
    #[error("not found: {message}")]
    NotFound {
        /// Human-readable message.
        message: String,
        /// Machine code field for the JSON body.
        code: &'static str,
    },

    /// Malformed input from the client. Renders as `400`.
    #[error("bad request: {message}")]
    BadRequest {
        /// Human-readable message.
        message: String,
        /// Machine code field for the JSON body.
        code: &'static str,
    },

    /// Caller asked to create a resource that already exists. Renders as
    /// `409`.
    #[error("conflict: {message}")]
    Conflict {
        /// Human-readable message.
        message: String,
        /// Machine code field for the JSON body.
        code: &'static str,
    },

    /// Required subsystem is not configured (e.g. dispatch route hit
    /// while `AppState.router` is `None`). Renders as `503`.
    #[error("service unavailable: {message}")]
    ServiceUnavailable {
        /// Human-readable message.
        message: String,
        /// Machine code field for the JSON body.
        code: &'static str,
    },

    /// Unexpected server-side failure. Renders as `500` with code
    /// `internal_error`; the underlying error is logged but not echoed
    /// to the wire so server-internal details do not leak.
    #[error("internal: {0}")]
    Internal(String),
}

impl RouteError {
    /// Construct a `404` with a machine code (e.g. `"adr_not_found"`).
    #[must_use]
    pub fn not_found(message: impl Into<String>, code: &'static str) -> Self {
        Self::NotFound {
            message: message.into(),
            code,
        }
    }

    /// Construct a `400` with a machine code (e.g. `"invalid_body"`).
    #[must_use]
    pub fn bad_request(message: impl Into<String>, code: &'static str) -> Self {
        Self::BadRequest {
            message: message.into(),
            code,
        }
    }

    /// Construct a `409` with a machine code.
    #[must_use]
    pub fn conflict(message: impl Into<String>, code: &'static str) -> Self {
        Self::Conflict {
            message: message.into(),
            code,
        }
    }

    /// Construct a `503` with a machine code.
    #[must_use]
    pub fn service_unavailable(message: impl Into<String>, code: &'static str) -> Self {
        Self::ServiceUnavailable {
            message: message.into(),
            code,
        }
    }

    /// Construct a `500`. The detail is logged server-side; the wire
    /// body always uses `code: "internal_error"` to avoid information
    /// leaks.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

/// JSON body shape — `{ "error": "...", "code": "..." }`.
///
/// `Serialize`-only by design; clients parse it via their own type.
#[derive(Debug, Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    code: &'a str,
}

impl IntoResponse for RouteError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::NotFound { message, code } => (StatusCode::NOT_FOUND, *code, message.clone()),
            Self::BadRequest { message, code } => (StatusCode::BAD_REQUEST, *code, message.clone()),
            Self::Conflict { message, code } => (StatusCode::CONFLICT, *code, message.clone()),
            Self::ServiceUnavailable { message, code } => {
                (StatusCode::SERVICE_UNAVAILABLE, *code, message.clone())
            }
            Self::Internal(detail) => {
                tracing::error!(detail = %detail, "route returned 500");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal server error".to_string(),
                )
            }
        };
        let body = ErrorBody {
            error: &message,
            code,
        };
        (status, Json(body)).into_response()
    }
}

/// Best-effort mapping from [`studio_store::StoreError`] into a
/// [`RouteError`]. NotFound stays 404 (via `is_not_found`); validation
/// failures (`InvalidInput`) become 400; conflicts (`AlreadyExists`)
/// become 409; everything else collapses to 500 with the underlying
/// error logged.
impl From<studio_store::StoreError> for RouteError {
    fn from(err: studio_store::StoreError) -> Self {
        use studio_store::StoreError;
        if err.is_not_found() {
            return Self::not_found(err.to_string(), "not_found");
        }
        match err {
            StoreError::InvalidInput(msg) => Self::bad_request(msg, "invalid_input"),
            StoreError::AlreadyExists(msg) => Self::conflict(msg, "already_exists"),
            StoreError::MissingFrontmatter(_) | StoreError::Frontmatter { .. } => {
                Self::bad_request(err.to_string(), "malformed_frontmatter")
            }
            other => Self::internal(other.to_string()),
        }
    }
}
