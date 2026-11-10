//! A gmail's browser, its proxy sealed.

use eyre::WrapErr;
use jiff::Timestamp;
use sqlx::SqliteConnection;

use super::{connections::Sealed, from_db, to_db};
pub use crate::store::connections::Checked;

/// Who runs a gmail's browser.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::Display, strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(serialize_all = "snake_case")]
pub enum Provider {
	Gologin,
}

/// Where the proxy comes out, as ip-api saw it at the last check through it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Exit {
	pub ip: String,
	/// ISO 3166-1 alpha-2.
	pub country: String,
	pub city: String,
	/// IANA.
	pub zone: String,
	/// A mobile carrier's.
	pub mobile: bool,
}

/// The provider's profile, made at the first start, and the noise it drew then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
	pub canvas: f64,
	pub webgl: f64,
	pub rects: f64,
	pub audio: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Browser {
	pub provider: Provider,
	pub device: sa_browsers::Device,
	/// host:port.
	pub endpoint: String,
	pub profile: Option<(String, Noise)>,
	pub exit: Exit,
	pub set_at: Timestamp,
	pub checked: Checked,
}

/// A proxy as stored: sealed, and its tag.
pub(crate) struct Proxy {
	pub sealed: Sealed,
	pub tag: [u8; 32],
	pub endpoint: String,
}

const COLUMNS: &str = "provider, device, endpoint, provider_profile, noise_canvas, noise_webgl, noise_rects, noise_audio, \
	exit_ip, exit_country, exit_city, exit_zone, exit_mobile, set_at, checked_at, check_error, proxy_sealed, data_key_fp";

#[derive(sqlx::FromRow)]
struct Row {
	provider: String,
	device: String,
	endpoint: String,
	provider_profile: Option<String>,
	noise_canvas: Option<f64>,
	noise_webgl: Option<f64>,
	noise_rects: Option<f64>,
	noise_audio: Option<f64>,
	exit_ip: String,
	exit_country: String,
	exit_city: String,
	exit_zone: String,
	exit_mobile: bool,
	set_at: i64,
	checked_at: i64,
	check_error: Option<String>,
	proxy_sealed: Vec<u8>,
	data_key_fp: Vec<u8>,
}

pub(crate) async fn browser(conn: &mut SqliteConnection, gmail_id: i64) -> eyre::Result<Option<(Browser, Sealed)>> {
	let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {COLUMNS} FROM gmail_browsers WHERE gmail_id = $1")))
		.bind(gmail_id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a gmail's browser")?;
	let Some(r) = row else {
		return Ok(None);
	};
	let profile = match (r.provider_profile, r.noise_canvas, r.noise_webgl, r.noise_rects, r.noise_audio) {
		(Some(id), Some(canvas), Some(webgl), Some(rects), Some(audio)) => Some((id, Noise { canvas, webgl, rects, audio })),
		(None, None, None, None, None) => None,
		half => eyre::bail!("gmail {gmail_id}'s browser profile is half there: {half:?}"),
	};
	let browser = Browser {
		provider: r.provider.parse()?,
		device: r.device.parse()?,
		endpoint: r.endpoint,
		profile,
		exit: Exit {
			ip: r.exit_ip,
			country: r.exit_country,
			city: r.exit_city,
			zone: r.exit_zone,
			mobile: r.exit_mobile,
		},
		set_at: from_db(r.set_at)?,
		checked: Checked {
			at: from_db(r.checked_at)?,
			error: r.check_error,
		},
	};
	Ok(Some((
		browser,
		Sealed {
			blob: r.proxy_sealed,
			data_key_fp: r.data_key_fp,
		},
	)))
}

/// The countries of the gmail's live GBPs that have one.
pub(crate) async fn gbp_countries(conn: &mut SqliteConnection, gmail_id: i64) -> eyre::Result<Vec<String>> {
	sqlx::query_scalar("SELECT DISTINCT country FROM gmbs WHERE gmail_id = $1 AND removed_at IS NULL AND country IS NOT NULL ORDER BY country")
		.bind(gmail_id)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing a gmail's GBPs' countries")
}

/// The gmail holding a proxy of this tag.
pub(crate) async fn tag_holder(conn: &mut SqliteConnection, tag: &[u8; 32]) -> eyre::Result<Option<i64>> {
	sqlx::query_scalar("SELECT gmail_id FROM gmail_browsers WHERE proxy_tag = $1")
		.bind(tag.as_slice())
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("looking a proxy up")
}

/// Checked when set: a refused one is never stored. Provider and device stay once set.
pub(crate) async fn set(conn: &mut SqliteConnection, gmail_id: i64, provider: Provider, device: sa_browsers::Device, proxy: &Proxy, exit: &Exit, now: Timestamp) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO gmail_browsers (gmail_id, provider, device, proxy_sealed, data_key_fp, proxy_tag, endpoint, exit_ip, exit_country, exit_city, \
		 exit_zone, exit_mobile, set_at, checked_at, check_error) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $13, NULL) \
		 ON CONFLICT (gmail_id) DO UPDATE SET proxy_sealed = excluded.proxy_sealed, data_key_fp = excluded.data_key_fp, proxy_tag = excluded.proxy_tag, \
		 endpoint = excluded.endpoint, exit_ip = excluded.exit_ip, exit_country = excluded.exit_country, exit_city = excluded.exit_city, \
		 exit_zone = excluded.exit_zone, exit_mobile = excluded.exit_mobile, set_at = excluded.set_at, checked_at = excluded.checked_at, check_error = NULL",
	)
	.bind(gmail_id)
	.bind(<&str>::from(provider))
	.bind(<&str>::from(device))
	.bind(&proxy.sealed.blob)
	.bind(&proxy.sealed.data_key_fp)
	.bind(proxy.tag.as_slice())
	.bind(&proxy.endpoint)
	.bind(&exit.ip)
	.bind(&exit.country)
	.bind(&exit.city)
	.bind(&exit.zone)
	.bind(exit.mobile)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err("setting a gmail's browser")?;
	Ok(())
}

/// What the last check saw: the exit when it went through, else the error, the exit kept.
pub(crate) async fn checked(conn: &mut SqliteConnection, gmail_id: i64, at: Timestamp, seen: Result<&Exit, &str>) -> eyre::Result<()> {
	let q = match seen {
		Ok(e) => sqlx::query(
			"UPDATE gmail_browsers SET checked_at = $2, check_error = NULL, exit_ip = $3, exit_country = $4, exit_city = $5, exit_zone = $6, exit_mobile = $7 WHERE gmail_id = $1",
		)
		.bind(gmail_id)
		.bind(to_db(at))
		.bind(&e.ip)
		.bind(&e.country)
		.bind(&e.city)
		.bind(&e.zone)
		.bind(e.mobile),
		Err(why) => sqlx::query("UPDATE gmail_browsers SET checked_at = $2, check_error = $3 WHERE gmail_id = $1")
			.bind(gmail_id)
			.bind(to_db(at))
			.bind(why),
	};
	q.execute(&mut *conn).await.wrap_err("recording a check of a gmail's browser")?;
	Ok(())
}

/// The provider's profile, made at the first start.
pub(crate) async fn set_profile(conn: &mut SqliteConnection, gmail_id: i64, id: &str, n: &Noise) -> eyre::Result<()> {
	sqlx::query("UPDATE gmail_browsers SET provider_profile = $2, noise_canvas = $3, noise_webgl = $4, noise_rects = $5, noise_audio = $6 WHERE gmail_id = $1")
		.bind(gmail_id)
		.bind(id)
		.bind(n.canvas)
		.bind(n.webgl)
		.bind(n.rects)
		.bind(n.audio)
		.execute(&mut *conn)
		.await
		.wrap_err("recording a browser's provider profile")?;
	Ok(())
}

/// `false`: none was set.
pub(crate) async fn delete(conn: &mut SqliteConnection, gmail_id: i64) -> eyre::Result<bool> {
	let done = sqlx::query("DELETE FROM gmail_browsers WHERE gmail_id = $1")
		.bind(gmail_id)
		.execute(&mut *conn)
		.await
		.wrap_err("deleting a gmail's browser")?;
	Ok(done.rows_affected() > 0)
}
