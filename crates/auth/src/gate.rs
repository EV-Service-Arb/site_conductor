//! The assertion's one verifier, and the gate a service's [`ev_lib::api::Route`] names
//! (docs/ARCHITECTURE.md, Invariants: a service trusts the conductor's assertion and nothing else).

use std::{
	convert::Infallible,
	future::{Future, ready},
	sync::Arc,
	time::{SystemTime, UNIX_EPOCH},
};

use axum::{
	extract::{FromRef, FromRequestParts, OptionalFromRequestParts, OriginalUri},
	http::{StatusCode, request::Parts},
};
use concierge_iam::Permission;
use ev_lib::api::{ErrorCode, Guard, OperationInput, Refusal};
use serde::Serialize;

use crate::{Assertion, HEADER, Keys, Principal, Service, Source, User};

/// Who passes a route's gate: a signed-in person holding `needs` (any, without one), and the
/// conductor where it admits it.
#[derive(Clone, Copy, Debug)]
pub struct Gate<F = ()> {
	needs: Option<&'static str>,
	user: bool,
	conductor: bool,
	/// What else the service decides per route (a timeout, a budget).
	pub flags: F,
}

impl Gate {
	/// The conductor's own assertion, and nobody else's.
	pub const CONDUCTOR: Self = Self {
		needs: None,
		user: false,
		conductor: true,
		flags: (),
	};
	pub const USER: Self = Self {
		needs: None,
		user: true,
		conductor: false,
		flags: (),
	};

	pub const fn with<F>(self, flags: F) -> Gate<F> {
		Gate {
			needs: self.needs,
			user: self.user,
			conductor: self.conductor,
			flags,
		}
	}
}

impl<F> Gate<F> {
	pub fn needs(self, permission: impl Permission) -> Self {
		let permission = permission.as_str();
		assert!(self.user, "{permission}: the conductor holds no permissions");
		assert!(self.needs.is_none(), "a gate names one permission: {permission} after {:?}", self.needs);
		Self { needs: Some(permission), ..self }
	}

	pub fn or_conductor(self) -> Self {
		Self { conductor: true, ..self }
	}
}

/// Why a gate refuses a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, strum::VariantArray)]
#[serde(rename_all = "snake_case")]
pub enum Denied {
	/// No assertion this service accepts for this request, or one of a caller the route does not admit.
	Unauthenticated,
	/// The caller does not hold the route's permission.
	Forbidden,
}

impl ErrorCode for Denied {
	fn status(self) -> StatusCode {
		match self {
			Self::Unauthenticated => StatusCode::UNAUTHORIZED,
			Self::Forbidden => StatusCode::FORBIDDEN,
		}
	}
}

/// Who a router's state says is calling: the conductor's assertion ([`Verifier`]), or, in the
/// conductor, its own session.
pub trait Vouch<F> {
	type Refusal: ErrorCode + From<Denied>;

	/// Every code [`Self::vouch`] refuses with.
	const REFUSALS: &'static [Self::Refusal];

	fn vouch(&self, gate: &Gate<F>, parts: &mut Parts) -> impl Future<Output = Result<Principal, Refusal<Self::Refusal>>> + Send;
}

impl<S, F> Vouch<F> for S
where
	Verifier: FromRef<S>,
{
	type Refusal = Denied;

	const REFUSALS: &'static [Denied] = &[Denied::Unauthenticated];

	fn vouch(&self, _: &Gate<F>, parts: &mut Parts) -> impl Future<Output = Result<Principal, Refusal<Denied>>> + Send {
		ready(Verifier::from_ref(self).principal(parts))
	}
}

impl<S: Vouch<F> + Send + Sync, F: Clone + Send + Sync + 'static> Guard<S> for Gate<F> {
	type Refusal = S::Refusal;

	fn permission(&self) -> Option<&'static str> {
		self.needs
	}

	fn refusals(&self) -> Vec<S::Refusal> {
		let gate = [Some(Denied::Unauthenticated), self.needs.map(|_| Denied::Forbidden)];
		S::REFUSALS.iter().copied().chain(gate.into_iter().flatten().map(Into::into)).collect()
	}

	async fn admit(&self, parts: &mut Parts, state: &S) -> Result<(), Refusal<S::Refusal>> {
		let refuse = |code: Denied, error: String| Refusal::new(code.into(), error);
		match state.vouch(self, parts).await? {
			Principal::User(user) if !self.user => Err(refuse(Denied::Unauthenticated, format!("{} does not open a route of the conductor's", user.email))),
			Principal::User(user) => {
				if let Some(needs) = self.needs
					&& !user.permissions.iter().any(|held| held == needs)
				{
					return Err(refuse(Denied::Forbidden, format!("{} does not hold {needs}", user.email)));
				}
				parts.extensions.insert(user);
				Ok(())
			}
			Principal::Conductor if self.conductor => Ok(()),
			Principal::Conductor => Err(refuse(Denied::Unauthenticated, "the conductor's own assertion does not open this route".into())),
			Principal::Source(source) => Err(refuse(Denied::Unauthenticated, format!("a source ({}) does not open this route", source.key_id))),
		}
	}
}

/// The conductor's public keys, and the service they are checked for.
#[derive(Clone, Debug)]
pub struct Verifier {
	keys: Arc<Keys>,
	aud: Service,
}

impl Verifier {
	pub fn new(keys: Keys, aud: Service) -> Self {
		Self { keys: Arc::new(keys), aud }
	}

	/// Who the request's assertion vouches for: what a service's own [`Vouch`] starts from.
	pub fn principal<C: ErrorCode + From<Denied>>(&self, parts: &Parts) -> Result<Principal, Refusal<C>> {
		self.assertion(parts, None).map(|a| a.principal)
	}

	/// The source the conductor checked the MAC of over `body`, which this request carries.
	pub fn source(&self, parts: &Parts, body: &[u8]) -> Result<Source, Refusal<Denied>> {
		match self.assertion(parts, Some(body))?.principal {
			Principal::Source(source) => Ok(source),
			_ => Err(unauthenticated("only a source's assertion opens this route".into())),
		}
	}

	/// The assertion on the request, for this service and for exactly its method and the path the
	/// conductor asked (a nested router sees it stripped).
	fn assertion<C: ErrorCode + From<Denied>>(&self, parts: &Parts, body: Option<&[u8]>) -> Result<Assertion, Refusal<C>> {
		let token = parts.headers.get(HEADER).ok_or_else(|| unauthenticated(format!("no {HEADER}")))?;
		let token = token.to_str().map_err(|_| unauthenticated(format!("{HEADER} is not ASCII")))?;
		let path = parts.extensions.get::<OriginalUri>().expect("the router records the URI it was asked").path();
		let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("the clock is past 1970").as_secs();
		let now = i64::try_from(now).expect("seconds since 1970 fit an i64");
		crate::verify(&self.keys, token, self.aud, parts.method.as_str(), path, body, now).map_err(|e| {
			tracing::warn!(method = %parts.method, path, reason = %e, "an assertion refused");
			unauthenticated(format!("the assertion is refused: {e}"))
		})
	}
}

/// The person a route's gate admitted.
impl<S: Send + Sync> FromRequestParts<S> for User {
	type Rejection = Infallible;

	async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
		Ok(parts
			.extensions
			.get::<User>()
			.expect("a route's gate puts the user it admitted; where it admits the conductor too, the handler takes Option<User>")
			.clone())
	}
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for User {
	type Rejection = Infallible;

	async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Option<Self>, Infallible> {
		Ok(parts.extensions.get::<User>().cloned())
	}
}

impl OperationInput for User {}

/// The conductor's own assertion: the gate of a handler outside the table, by its signature.
#[derive(Clone, Copy, Debug)]
pub struct Conductor;

impl<S: Vouch<()> + Send + Sync> FromRequestParts<S> for Conductor {
	type Rejection = Refusal<S::Refusal>;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Refusal<S::Refusal>> {
		match state.vouch(&Gate::CONDUCTOR, parts).await? {
			Principal::Conductor => Ok(Self),
			_ => Err(unauthenticated("only the conductor's own assertion opens this route".into())),
		}
	}
}

fn unauthenticated<C: ErrorCode + From<Denied>>(error: String) -> Refusal<C> {
	Refusal::new(Denied::Unauthenticated.into(), error)
}
