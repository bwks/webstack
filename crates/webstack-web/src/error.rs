use std::{error::Error, sync::Arc};

use askama::Template;
use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

const HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8";
const TEXT_CONTENT_TYPE: &str = "text/plain; charset=utf-8";
const TEMPLATE_FAILURE_MESSAGE: &str = "The error response could not be rendered.";

/// Application-provided rendering hook for safe framework errors.
pub type ErrorRenderer = Arc<dyn Fn(&ErrorView) -> Option<String> + Send + Sync>;

/// Safe data made available to application-owned error templates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorView {
    status: StatusCode,
    title: &'static str,
    message: String,
    is_htmx: bool,
}

impl ErrorView {
    /// Returns the response status associated with this error.
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns a short, safe title for this error category.
    #[must_use]
    pub const fn title(&self) -> &'static str {
        self.title
    }

    /// Returns the safe public message for the response.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns whether the response is being rendered for htmx.
    #[must_use]
    pub const fn is_htmx(&self) -> bool {
        self.is_htmx
    }

    /// Returns a copy adjusted for the current request protocol.
    #[must_use]
    pub(crate) fn with_htmx(&self, is_htmx: bool) -> Self {
        let mut view = self.clone();
        view.is_htmx = is_htmx;
        view
    }
}

/// A request-level failure with a safe public representation.
#[derive(Debug, thiserror::Error)]
#[error("{kind}")]
pub struct AppError {
    #[source]
    kind: AppErrorKind,
}

impl AppError {
    /// Creates a safe `400 Bad Request` failure.
    #[must_use]
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            kind: AppErrorKind::BadRequest(message.into()),
        }
    }

    /// Creates a safe `403 Forbidden` failure.
    #[must_use]
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            kind: AppErrorKind::Forbidden(message.into()),
        }
    }

    /// Creates a safe `404 Not Found` failure.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            kind: AppErrorKind::NotFound(message.into()),
        }
    }

    /// Creates a safe `409 Conflict` failure.
    #[must_use]
    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: AppErrorKind::Conflict(message.into()),
        }
    }

    /// Creates a safe `422 Unprocessable Entity` validation failure.
    #[must_use]
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: AppErrorKind::Validation(message.into()),
        }
    }

    /// Creates an internal failure whose source is logged but never rendered.
    pub fn internal(operation: &'static str, source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            kind: AppErrorKind::Internal {
                operation,
                source: Box::new(source),
            },
        }
    }

    /// Converts this failure into safe template data.
    #[must_use]
    pub(crate) fn view(&self) -> ErrorView {
        let (status, title, message) = match &self.kind {
            AppErrorKind::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, "Bad request", message.clone())
            }
            AppErrorKind::Forbidden(message) => {
                (StatusCode::FORBIDDEN, "Forbidden", message.clone())
            }
            AppErrorKind::NotFound(message) => {
                (StatusCode::NOT_FOUND, "Not found", message.clone())
            }
            AppErrorKind::Conflict(message) => (StatusCode::CONFLICT, "Conflict", message.clone()),
            AppErrorKind::Validation(message) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Validation failed",
                message.clone(),
            ),
            AppErrorKind::Internal { operation, source } => {
                tracing::error!(%operation, %source, "application request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error",
                    "The request could not be completed.".to_owned(),
                )
            }
        };
        ErrorView {
            status,
            title,
            message,
            is_htmx: false,
        }
    }
}

impl IntoResponse for AppError {
    /// Produces a safe default response and marks it for framework rendering.
    fn into_response(self) -> Response {
        let view = self.view();
        let mut response = render_error(&view, None);
        response.extensions_mut().insert(ErrorMarker(view));
        response
    }
}

#[derive(Debug, thiserror::Error)]
enum AppErrorKind {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("{operation} failed")]
    Internal {
        operation: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ErrorMarker(pub(crate) ErrorView);

#[derive(Template)]
#[template(path = "errors/page.html.jinja", ext = "html")]
struct DefaultErrorPageTemplate<'a> {
    view: &'a ErrorView,
}

#[derive(Template)]
#[template(path = "errors/partial.html.jinja", ext = "html")]
struct DefaultErrorPartialTemplate<'a> {
    view: &'a ErrorView,
}

/// Renders a marked application error with a custom hook or framework fallback.
pub(crate) fn render_error(view: &ErrorView, renderer: Option<&ErrorRenderer>) -> Response {
    let (body, content_type) = renderer.and_then(|render| render(view)).map_or_else(
        || render_default_html(view),
        |html| (html, HTML_CONTENT_TYPE),
    );
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = view.status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(content_type),
    );
    response
}

/// Renders the framework's escaped full-page or fragment fallback template.
fn render_default_html(view: &ErrorView) -> (String, &'static str) {
    let rendered = if view.is_htmx {
        DefaultErrorPartialTemplate { view }.render()
    } else {
        DefaultErrorPageTemplate { view }.render()
    };
    match rendered {
        Ok(html) => (html, HTML_CONTENT_TYPE),
        Err(source) => {
            tracing::error!(%source, "framework error template rendering failed");
            (TEMPLATE_FAILURE_MESSAGE.to_owned(), TEXT_CONTENT_TYPE)
        }
    }
}
