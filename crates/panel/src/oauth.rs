//! MCP clients (RFC 7591) and what a user's consent grants one: a session of its own — rotated,
//! refused and closed as a browser's — whose id the client holds as its bearer.

use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
	Panel,
	session::{Tokens, pkce_challenge, random_token},
	store::{oauth, sessions},
};

/// How long a code waits for its client: it is presented at once, from the redirect.
pub const CODE_TTL: SignedDuration = SignedDuration::from_secs(60);

/// What a user consented to, carried through concierge's sign-in to the callback.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Grant {
	pub user_id: Uuid,
	pub client_id: String,
	pub redirect_uri: String,
	/// The client's PKCE challenge (S256).
	pub challenge: String,
	/// The client's `state`, handed back with the code.
	pub state: Option<String>,
}

/// A registered client.
pub struct Client {
	pub redirect_uris: Vec<String>,
	pub name: Option<String>,
}

fn code_hash(code: &str) -> [u8; 32] {
	Sha256::new().chain_update(b"sa-panel/oauth-code/v1/").chain_update(code.as_bytes()).finalize().into()
}

fn bearer_aad(code_hash: &[u8; 32]) -> Vec<u8> {
	[b"sa-panel/oauth-bearer/v1/".as_slice(), code_hash].concat()
}

impl Panel {
	/// Its client id.
	pub async fn register_client(&self, redirect_uris: &[String], name: Option<&str>, now: Timestamp) -> eyre::Result<String> {
		let id = random_token()?;
		let uris = serde_json::to_string(redirect_uris).wrap_err("serializing redirect URIs")?;
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an MCP client")?;
		oauth::insert_client(&mut conn, &id, &uris, name, now).await?;
		Ok(id)
	}

	pub async fn client(&self, client_id: &str) -> eyre::Result<Option<Client>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an MCP client")?;
		let Some((uris, name)) = oauth::client(&mut conn, client_id).await? else {
			return Ok(None);
		};
		let redirect_uris = serde_json::from_str(&uris).wrap_err("a client's redirect URIs")?;
		Ok(Some(Client { redirect_uris, name }))
	}

	/// Opens the session `grant` gives its client, on the tokens concierge issued its user; the
	/// code that hands the client its bearer.
	pub async fn grant(&self, grant: &Grant, tokens: &Tokens, now: Timestamp) -> eyre::Result<String> {
		let (key, opened) = self.open(grant.user_id, Some(&grant.client_id), tokens, now).await?;
		let code = random_token()?;
		let hash = code_hash(&code);
		let sealed = self.key.seal(&bearer_aad(&hash), opened.cookie.as_bytes())?;
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an MCP code")?;
		oauth::prune_codes(&mut conn, now).await?;
		let issued = oauth::CodeRow {
			session: key.as_bytes().to_vec(),
			redirect_uri: grant.redirect_uri.clone(),
			challenge: grant.challenge.clone(),
			bearer_sealed: sealed,
			data_key_fp: self.key.fingerprint().to_vec(),
			expires_at: now + CODE_TTL,
		};
		oauth::insert_code(&mut conn, &hash, &issued).await?;
		Ok(code)
	}

	/// The bearer `code` hands `client_id`, when its redirect URI and verifier are the code's and
	/// its minute has not passed. The code is spent whatever the answer; a code presented wrong
	/// closes its session.
	pub async fn redeem(&self, code: &str, client_id: &str, redirect_uri: &str, verifier: &str, now: Timestamp) -> eyre::Result<Option<Zeroizing<String>>> {
		let hash = code_hash(code);
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an MCP code")?;
		let Some(row) = oauth::take_code(&mut conn, &hash).await? else {
			return Ok(None);
		};
		let Some(session) = sessions::get(&mut conn, &row.session).await? else {
			return Ok(None);
		};
		let ok = row.expires_at > now && row.redirect_uri == redirect_uri && session.client_id.as_deref() == Some(client_id) && pkce_challenge(verifier) == row.challenge;
		if !ok {
			sessions::delete(&mut conn, &row.session).await?;
			return Ok(None);
		}
		eyre::ensure!(row.data_key_fp == self.key.fingerprint(), "an MCP code was sealed under another PANEL_DATA_KEY");
		let bearer = self.key.open(&bearer_aad(&hash), &row.bearer_sealed)?;
		Ok(Some(Zeroizing::new(String::from_utf8(bearer.to_vec()).wrap_err("a sealed bearer is not UTF-8")?)))
	}
}
