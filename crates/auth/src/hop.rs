//! What the conductor answers on a forwarded path itself, before the service is asked or in its place:
//! a service's client takes these beside its own document's refusals.

use axum::http::StatusCode;
use ev_lib::api::ErrorCode;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, strum::VariantArray)]
#[serde(rename_all = "snake_case")]
pub enum Hop {
	/// A path that would climb out of the service's prefix.
	BadRequest,
	/// No session, or one closed: sign in again.
	Unauthenticated,
	/// A cookie's write without the page's CSRF header.
	Csrf,
	/// Concierge, or the service, cannot be asked now.
	Unavailable,
	/// The service did not answer.
	BadGateway,
	/// The conductor's own failure.
	Internal,
}

impl ErrorCode for Hop {
	fn status(self) -> StatusCode {
		match self {
			Self::BadRequest => StatusCode::BAD_REQUEST,
			Self::Unauthenticated => StatusCode::UNAUTHORIZED,
			Self::Csrf => StatusCode::FORBIDDEN,
			Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
			Self::BadGateway => StatusCode::BAD_GATEWAY,
			Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
		}
	}
}
