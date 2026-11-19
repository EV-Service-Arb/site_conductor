//! Google's side of a gmail's Business Profile connection: its OAuth client, the consent that
//! grants `business.manage` to it, and the Business Profile APIs it opens. A refusal is Google's
//! words, for the person; a failure to reach Google is an error.
//!
//! ```text
//! set client ─ a code exchange with no code: invalid_grant means Google knows the client
//! consent    ─ accounts.google.com, login_hint the gmail ─▶ redirect_uri?code&state
//! grant      ─ code ─▶ refresh token + id_token, whose email must be the gmail's
//! check      ─ refresh ─▶ access ─▶ accounts.list: 429 at 0 QPM until Google approves the project
//! ```

use std::time::Duration;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use eyre::WrapErr;
use serde::Deserialize;
use zeroize::Zeroizing;

const AUTH: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN: &str = "https://oauth2.googleapis.com/token";
const ACCOUNTS: &str = "https://mybusinessaccountmanagement.googleapis.com/v1/accounts";
const SCOPE: &str = "openid email https://www.googleapis.com/auth/business.manage";

pub(crate) struct Client<'a> {
	pub id: &'a str,
	pub secret: &'a str,
}

#[derive(Deserialize)]
struct TokenError {
	error: String,
	error_description: Option<String>,
}
impl TokenError {
	fn said(self) -> String {
		match self.error_description {
			Some(d) => format!("{}: {d}", self.error),
			None => self.error,
		}
	}
}

#[derive(Deserialize)]
struct Tokens {
	access_token: String,
	refresh_token: Option<String>,
	id_token: Option<String>,
}

pub(crate) struct Granted {
	pub refresh: Zeroizing<String>,
	pub email: String,
}

fn http() -> eyre::Result<reqwest::Client> {
	reqwest::Client::builder().timeout(Duration::from_secs(20)).build().wrap_err("an HTTP client for Google's OAuth")
}

async fn token(form: &[(&str, &str)]) -> eyre::Result<Result<Tokens, String>> {
	let res = http()?.post(TOKEN).form(form).send().await.wrap_err("asking Google's token endpoint")?;
	if res.status().is_success() {
		return Ok(Ok(res.json().await.wrap_err("Google's token answer")?));
	}
	let status = res.status();
	let body = res.text().await.wrap_err("Google's token refusal")?;
	let e: TokenError = serde_json::from_str(&body).wrap_err_with(|| format!("Google's token refusal, {status}: {body}"))?;
	Ok(Err(e.said()))
}

/// Whether Google knows the client and its secret.
pub(crate) async fn probe(client: &Client<'_>, redirect_uri: &str) -> eyre::Result<Result<(), String>> {
	let form = [
		("grant_type", "authorization_code"),
		("code", "probe"),
		("client_id", client.id),
		("client_secret", client.secret),
		("redirect_uri", redirect_uri),
	];
	Ok(match token(&form).await? {
		Ok(_) => unreachable!("Google takes no code \"probe\""),
		Err(said) if said.starts_with("invalid_grant") => Ok(()),
		Err(said) => Err(said),
	})
}

pub(crate) fn consent(client_id: &str, redirect_uri: &str, gmail: &str, state: &str) -> String {
	let mut url = url::Url::parse(AUTH).expect("a constant URL");
	url.query_pairs_mut()
		.append_pair("client_id", client_id)
		.append_pair("redirect_uri", redirect_uri)
		.append_pair("response_type", "code")
		.append_pair("scope", SCOPE)
		.append_pair("access_type", "offline")
		// a refresh token comes only with a consent shown
		.append_pair("prompt", "consent")
		.append_pair("login_hint", gmail)
		.append_pair("state", state);
	url.into()
}

pub(crate) async fn exchange(client: &Client<'_>, code: &str, redirect_uri: &str) -> eyre::Result<Result<Granted, String>> {
	let form = [
		("grant_type", "authorization_code"),
		("code", code),
		("client_id", client.id),
		("client_secret", client.secret),
		("redirect_uri", redirect_uri),
	];
	let t = match token(&form).await? {
		Ok(t) => t,
		Err(said) => return Ok(Err(said)),
	};
	let refresh = t
		.refresh_token
		.ok_or_else(|| eyre::eyre!("Google granted with prompt=consent and access_type=offline, and sent no refresh token"))?;
	let id_token = t.id_token.ok_or_else(|| eyre::eyre!("Google granted the openid scope and sent no id_token"))?;
	// straight from the token endpoint over TLS: its signature need not be checked (OIDC Core 3.1.3.7)
	let payload = id_token.split('.').nth(1).ok_or_else(|| eyre::eyre!("an id_token without a payload"))?;
	#[derive(Deserialize)]
	struct Claims {
		email: String,
	}
	let claims: Claims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).wrap_err("an id_token's payload")?).wrap_err("an id_token's claims")?;
	Ok(Ok(Granted {
		refresh: Zeroizing::new(refresh),
		email: claims.email,
	}))
}

/// What the Business Profile APIs answer the grant: `None` they opened.
pub(crate) async fn check(client: &Client<'_>, refresh: &str) -> eyre::Result<Option<String>> {
	let form = [
		("grant_type", "refresh_token"),
		("refresh_token", refresh),
		("client_id", client.id),
		("client_secret", client.secret),
	];
	let access = match token(&form).await? {
		Ok(t) => Zeroizing::new(t.access_token),
		Err(said) => return Ok(Some(said)),
	};
	let res = http()?.get(ACCOUNTS).bearer_auth(access.as_str()).send().await.wrap_err("asking the Business Profile APIs")?;
	if res.status().is_success() {
		return Ok(None);
	}
	let status = res.status();
	let body = res.text().await.wrap_err("the Business Profile APIs' refusal")?;
	#[derive(Deserialize)]
	struct Refusal {
		error: Said,
	}
	#[derive(Deserialize)]
	struct Said {
		message: String,
	}
	let r: Refusal = serde_json::from_str(&body).wrap_err_with(|| format!("the Business Profile APIs' refusal, {status}: {body}"))?;
	Ok(Some(format!("{status}: {}", r.error.message)))
}
