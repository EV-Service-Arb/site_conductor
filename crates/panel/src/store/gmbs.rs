//! People's gmails and the GBPs each manages. Never deleted: a removed row keeps its id, which
//! the services hold.

use std::collections::BTreeSet;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::gbp::{Country, Managed, State};
use sa_places::LatLng;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};

const GMB: &str = "SELECT b.id, b.gmail_id, b.place_id, b.name, b.address, b.state, b.country, b.country_by IS NOT NULL AS country_by_hand, b.phone, \
	b.lat, b.lng, b.placed_by IS NOT NULL AS placed, b.created_at, NOT EXISTS (SELECT 1 FROM gmbs_off o WHERE o.gmb_id = b.id) AS enabled FROM gmbs b";

type GmailRow = (i64, Uuid, String, i64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gmail {
	pub id: i64,
	pub owner: Uuid,
	pub gmail: String,
	pub created_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gmb {
	pub id: i64,
	/// `None`: a draft no gmail manages yet.
	pub gmail_id: Option<i64>,
	/// `None` while a draft, and only then.
	pub place_id: Option<String>,
	pub name: String,
	pub address: Option<String>,
	pub state: State,
	pub country: Option<GbpCountry>,
	pub phone: Option<String>,
	pub created_at: Timestamp,
	/// Its owner's switch.
	pub enabled: bool,
	/// `None`: none on Google (a service-area place, a draft) and none placed yet, or not located
	/// since it was added without a Places key.
	pub location: Option<Point>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Point {
	#[serde(flatten)]
	pub at: LatLng,
	/// Placed by hand on the map, Google having none.
	pub placed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, ts_rs::TS)]
pub struct GbpCountry {
	pub code: Country,
	/// Set by hand, Google having none.
	pub by_hand: bool,
}

/// What a new GBP row holds; `gmail_id` and `place_id` absent for a draft.
pub struct NewGmb<'a> {
	pub owner: Uuid,
	pub gmail_id: Option<i64>,
	pub place_id: Option<&'a str>,
	pub name: &'a str,
	pub address: Option<&'a str>,
	pub state: State,
	pub country: Option<Country>,
	/// Set by hand, not Google's.
	pub country_by_hand: bool,
	/// Google's.
	pub at: Option<LatLng>,
}

/// The owner's gmails not removed, by address.
pub async fn gmails(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<Gmail>> {
	let rows: Vec<GmailRow> = sqlx::query_as("SELECT id, owner, gmail, created_at FROM gmails WHERE owner = $1 AND removed_at IS NULL ORDER BY gmail")
		.bind(owner)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing gmails")?;
	rows.into_iter().map(gmail).collect()
}

/// The owner's GBPs not removed, oldest first.
pub async fn gmbs(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<Gmb>> {
	let rows: Vec<GmbRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{GMB} WHERE b.owner = $1 AND b.removed_at IS NULL ORDER BY b.id")))
		.bind(owner)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing GMBs")?;
	rows.into_iter().map(gmb).collect()
}

/// GBP `id`, not removed, of `owner`'s unless `None` (the operator's CLI).
pub async fn gmb_of(conn: &mut SqliteConnection, owner: Option<Uuid>, id: i64) -> eyre::Result<Option<Gmb>> {
	let row: Option<GmbRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{GMB} WHERE b.id = $1 AND ($2 IS NULL OR b.owner = $2) AND b.removed_at IS NULL")))
		.bind(id)
		.bind(owner)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a GMB")?;
	row.map(gmb).transpose()
}

/// `id` explicit only on import. `None`: the owner has that gmail already.
pub async fn add_gmail(conn: &mut SqliteConnection, id: Option<i64>, owner: Uuid, address: &str, now: Timestamp) -> eyre::Result<Option<Gmail>> {
	let row: Option<GmailRow> = sqlx::query_as(
		"INSERT INTO gmails (id, owner, gmail, created_at) VALUES ($1, $2, $3, $4) \
		 ON CONFLICT DO NOTHING RETURNING id, owner, gmail, created_at",
	)
	.bind(id)
	.bind(owner)
	.bind(address)
	.bind(to_db(now))
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("adding a gmail")?;
	row.map(gmail).transpose()
}

/// The gmail removed, and the GBPs it manages with it. `false`: no such gmail of the owner's, or
/// it is removed already.
pub async fn remove_gmail(conn: &mut SqliteConnection, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE gmails SET removed_at = $3 WHERE id = $1 AND owner = $2 AND removed_at IS NULL")
		.bind(id)
		.bind(owner)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("removing a gmail")?;
	if done.rows_affected() == 0 {
		return Ok(false);
	}
	sqlx::query("UPDATE gmbs SET removed_at = $2 WHERE gmail_id = $1 AND removed_at IS NULL")
		.bind(id)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("removing a gmail's GMBs")?;
	Ok(true)
}

/// The owner of a gmail not removed.
pub async fn gmail_owner(conn: &mut SqliteConnection, id: i64) -> eyre::Result<Option<Uuid>> {
	sqlx::query_scalar("SELECT owner FROM gmails WHERE id = $1 AND removed_at IS NULL")
		.bind(id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a gmail")
}

/// Why `name` or `place_id` cannot be GBP `except`'s: another live GBP has it.
pub async fn taken(conn: &mut SqliteConnection, name: Option<&str>, place_id: Option<&str>, except: Option<i64>) -> eyre::Result<Option<String>> {
	let (by_name, by_place): (Option<i64>, Option<i64>) = sqlx::query_as(
		"SELECT (SELECT id FROM gmbs WHERE name = $1 AND removed_at IS NULL AND id IS NOT $3), \
		 (SELECT id FROM gmbs WHERE place_id = $2 AND removed_at IS NULL AND id IS NOT $3)",
	)
	.bind(name)
	.bind(place_id)
	.bind(except)
	.fetch_one(&mut *conn)
	.await
	.wrap_err("looking for a GMB's name and place")?;
	Ok(match (by_name, by_place) {
		(Some(id), _) => Some(format!("GBP {id} is named {:?} already", name.expect("matched by it"))),
		(None, Some(id)) => Some(format!("GBP {id} is on {} already", place_id.expect("matched by it"))),
		(None, None) => None,
	})
}

/// `id` explicit only on import. Its name and place are free ([`taken`]).
pub async fn add_gmb(conn: &mut SqliteConnection, id: Option<i64>, new: &NewGmb<'_>, now: Timestamp) -> eyre::Result<Gmb> {
	let added: i64 = sqlx::query_scalar(
		"INSERT INTO gmbs (id, owner, gmail_id, place_id, name, address, state, country, country_by, lat, lng, created_at) \
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, CASE WHEN $9 THEN $2 END, $10, $11, $12) RETURNING id",
	)
	.bind(id)
	.bind(new.owner)
	.bind(new.gmail_id)
	.bind(new.place_id)
	.bind(new.name)
	.bind(new.address)
	.bind(new.state.as_str())
	.bind(new.country.map(Country::as_str))
	.bind(new.country_by_hand)
	.bind(new.at.map(|a| a.lat))
	.bind(new.at.map(|a| a.lng))
	.bind(to_db(now))
	.fetch_one(&mut *conn)
	.await
	.wrap_err("adding a GMB")?;
	Ok(gmb_of(conn, None, added).await?.expect("added just now"))
}

/// A draft's name; the schema refuses it of any other GBP.
pub async fn rename(conn: &mut SqliteConnection, id: i64, name: &str) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET name = $2 WHERE id = $1")
		.bind(id)
		.bind(name)
		.execute(&mut *conn)
		.await
		.wrap_err("renaming a GMB")?;
	Ok(())
}

/// To another gmail of its owner's: the schema refuses anyone else's.
pub async fn move_to(conn: &mut SqliteConnection, id: i64, gmail_id: i64) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET gmail_id = $2 WHERE id = $1")
		.bind(id)
		.bind(gmail_id)
		.execute(&mut *conn)
		.await
		.wrap_err("moving a GMB")?;
	Ok(())
}

pub async fn set_phone(conn: &mut SqliteConnection, id: i64, phone: Option<&str>) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET phone = $2 WHERE id = $1")
		.bind(id)
		.bind(phone)
		.execute(&mut *conn)
		.await
		.wrap_err("setting a GMB's phone")?;
	Ok(())
}

/// GBP `id` in `to`; the schema checks the move against [`State::next`] again.
pub async fn set_state(conn: &mut SqliteConnection, id: i64, to: State) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET state = $2 WHERE id = $1")
		.bind(id)
		.bind(to.as_str())
		.execute(&mut *conn)
		.await
		.wrap_err("moving a GMB's state")?;
	Ok(())
}

/// A draft onto Google: its place, Google's address, and Google's point and country over those
/// set by hand.
pub async fn place_draft(conn: &mut SqliteConnection, id: i64, place_id: &str, address: Option<&str>, at: Option<LatLng>, country: Option<Country>) -> eyre::Result<()> {
	// country first: the schema refuses a placed GBP without one
	if let Some(c) = country {
		set_country(conn, id, c).await?;
	}
	if let Some(at) = at {
		set_point(conn, id, at).await?;
	}
	sqlx::query("UPDATE gmbs SET place_id = $2, address = $3, state = 'verifying' WHERE id = $1")
		.bind(id)
		.bind(place_id)
		.bind(address)
		.execute(&mut *conn)
		.await
		.wrap_err("placing a draft on Google")?;
	Ok(())
}

/// Where GBP `id` is, as Google answered last: over a point placed by hand too, the place a
/// storefront now.
pub async fn set_point(conn: &mut SqliteConnection, id: i64, at: LatLng) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET lat = $2, lng = $3, placed_by = NULL WHERE id = $1")
		.bind(id)
		.bind(at.lat)
		.bind(at.lng)
		.execute(&mut *conn)
		.await
		.wrap_err("locating a GMB")?;
	Ok(())
}

/// The owner's GBP `gmb_id` placed at `at` by them. `Err(NotFound)`: not their GBP, or removed;
/// `Err(Conflict)`: Google has a point for it.
pub async fn place(conn: &mut SqliteConnection, owner: Uuid, gmb_id: i64, at: LatLng) -> eyre::Result<Result<(), crate::Refused>> {
	let Some(gbp) = gmb_of(conn, Some(owner), gmb_id).await? else {
		return Ok(Err(crate::Refused::NotFound));
	};
	if gbp.location.is_some_and(|p| !p.placed) {
		return Ok(Err(crate::Refused::Conflict(format!("Google places GBP {gmb_id} itself"))));
	}
	sqlx::query("UPDATE gmbs SET lat = $2, lng = $3, placed_by = $4 WHERE id = $1")
		.bind(gmb_id)
		.bind(at.lat)
		.bind(at.lng)
		.bind(owner)
		.execute(&mut *conn)
		.await
		.wrap_err("placing a point")?;
	Ok(Ok(()))
}

/// The live GBPs on Google without a point or a country of Google's: each id, its place, and
/// whether it has no point.
pub async fn unlocated(conn: &mut SqliteConnection) -> eyre::Result<Vec<(i64, String, bool)>> {
	sqlx::query_as(
		"SELECT id, place_id, lat IS NULL FROM gmbs \
		 WHERE removed_at IS NULL AND place_id IS NOT NULL AND (lat IS NULL OR country IS NULL OR country_by IS NOT NULL) ORDER BY id",
	)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing the GMBs without a point or a country")
}

/// Google's country for GBP `id`, over one set by hand.
pub async fn set_country(conn: &mut SqliteConnection, id: i64, country: Country) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET country = $2, country_by = NULL WHERE id = $1")
		.bind(id)
		.bind(country.as_str())
		.execute(&mut *conn)
		.await
		.wrap_err("setting a GMB's country")?;
	Ok(())
}

/// GBP `gmb_id` in `country`, set by hand by its owner, who must be `owner` unless `None` (the
/// operator's CLI). `Err(NotFound)`: not theirs, or removed; `Err(Conflict)`: Google has its
/// country.
pub async fn set_country_by_hand(conn: &mut SqliteConnection, owner: Option<Uuid>, gmb_id: i64, country: Country) -> eyre::Result<Result<(), crate::Refused>> {
	let Some(gbp) = gmb_of(conn, owner, gmb_id).await? else {
		return Ok(Err(crate::Refused::NotFound));
	};
	if gbp.country.is_some_and(|c| !c.by_hand) {
		return Ok(Err(crate::Refused::Conflict(format!("Google gives GBP {gmb_id} its country"))));
	}
	sqlx::query("UPDATE gmbs SET country = $2, country_by = owner WHERE id = $1")
		.bind(gmb_id)
		.bind(country.as_str())
		.execute(&mut *conn)
		.await
		.wrap_err("setting a GMB's country")?;
	Ok(Ok(()))
}

/// `false`: no such GBP of the owner's, or it is removed already.
pub async fn remove_gmb(conn: &mut SqliteConnection, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE gmbs SET removed_at = $3 WHERE id = $1 AND owner = $2 AND removed_at IS NULL")
		.bind(id)
		.bind(owner)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("removing a GMB")?;
	Ok(done.rows_affected() == 1)
}

/// Each of `ids` on or off, or none of them: `false` when one is no GBP of the owner's not
/// removed.
pub async fn switch(conn: &mut SqliteConnection, owner: Uuid, ids: &BTreeSet<i64>, on: bool) -> eyre::Result<bool> {
	let asked = i64::try_from(ids.len()).expect("a few ids");
	let ids = serde_json::to_string(ids).expect("ids serialize");
	let theirs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM gmbs WHERE id IN (SELECT value FROM json_each($1)) AND owner = $2 AND removed_at IS NULL")
		.bind(&ids)
		.bind(owner)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("checking whose GMBs these are")?;
	if theirs != asked {
		return Ok(false);
	}
	let sql = match on {
		true => "DELETE FROM gmbs_off WHERE gmb_id IN (SELECT value FROM json_each($1))",
		// `WHERE true`: SQLite would read ON CONFLICT as the SELECT's join
		false => "INSERT INTO gmbs_off (gmb_id) SELECT value FROM json_each($1) WHERE true ON CONFLICT DO NOTHING",
	};
	sqlx::query(sql).bind(&ids).execute(&mut *conn).await.wrap_err("switching GMBs")?;
	Ok(true)
}

/// Every GBP that ever had a place, by id.
pub async fn managed(conn: &mut SqliteConnection) -> eyre::Result<Vec<Managed>> {
	type Row = (i64, Option<i64>, Option<String>, Uuid, bool, String, bool, String, Option<String>, String, Option<String>);
	let rows: Vec<Row> = sqlx::query_as(
		"SELECT b.id, b.gmail_id, g.gmail, b.owner, b.removed_at IS NOT NULL OR g.removed_at IS NOT NULL, b.place_id, \
		 NOT EXISTS (SELECT 1 FROM gmbs_off o WHERE o.gmb_id = b.id), b.state, b.country, b.name, b.address \
		 FROM gmbs b LEFT JOIN gmails g ON g.id = b.gmail_id WHERE b.place_id IS NOT NULL ORDER BY b.id",
	)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing every GMB")?;
	rows.into_iter()
		.map(|(id, gmail_id, gmail, owner, removed, place_id, enabled, state, country, name, address)| {
			let (Some(gmail_id), Some(gmail)) = (gmail_id, gmail) else {
				eyre::bail!("GMB {id} is on {place_id} with no gmail, which the schema refuses");
			};
			let country = country.as_deref().map(str::parse::<Country>).transpose()?;
			eyre::ensure!(removed || country.is_some(), "GMB {id} is live on {place_id} with no country, which the schema refuses");
			Ok(Managed {
				id,
				gmail_id,
				gmail,
				owner,
				removed,
				place_id,
				enabled,
				state: state.parse()?,
				country,
				name,
				address,
			})
		})
		.collect()
}

#[derive(sqlx::FromRow)]
struct GmbRow {
	id: i64,
	gmail_id: Option<i64>,
	place_id: Option<String>,
	name: String,
	address: Option<String>,
	state: String,
	country: Option<String>,
	country_by_hand: bool,
	phone: Option<String>,
	lat: Option<f64>,
	lng: Option<f64>,
	placed: bool,
	created_at: i64,
	enabled: bool,
}

fn gmail((id, owner, gmail, created_at): GmailRow) -> eyre::Result<Gmail> {
	Ok(Gmail {
		id,
		owner,
		gmail,
		created_at: from_db(created_at)?,
	})
}

fn gmb(r: GmbRow) -> eyre::Result<Gmb> {
	let location = match (r.lat, r.lng) {
		(Some(lat), Some(lng)) => Some(Point {
			at: LatLng { lat, lng },
			placed: r.placed,
		}),
		(None, None) => None,
		half => eyre::bail!("GMB {}'s point is half there: {half:?}", r.id),
	};
	Ok(Gmb {
		id: r.id,
		gmail_id: r.gmail_id,
		place_id: r.place_id,
		name: r.name,
		address: r.address,
		state: r.state.parse()?,
		country: r.country.as_deref().map(str::parse).transpose()?.map(|code| GbpCountry { code, by_hand: r.country_by_hand }),
		phone: r.phone,
		created_at: from_db(r.created_at)?,
		enabled: r.enabled,
		location,
	})
}
