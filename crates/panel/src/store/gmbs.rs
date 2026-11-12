//! People's gmails and the GBPs each manages. Never deleted: a removed row keeps its id, which
//! the services hold.

use std::collections::BTreeSet;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::gbp::{Country, GmailRole, Managed, State};
use sa_places::LatLng;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};
use crate::google_categories::GoogleCategory;

const GMB: &str = "SELECT b.id, b.gmail_id, b.place_id, b.name, b.address, b.state, b.country, b.country_by IS NOT NULL AS country_by_hand, b.phone, \
	b.trade_id, b.areas, b.hidden_address, b.customers_visit, b.film, b.lat, b.lng, b.placed_by IS NOT NULL AS placed, b.created_at, NOT EXISTS (SELECT 1 FROM gmbs_off o WHERE o.gmb_id = b.id) AS enabled FROM gmbs b";

/// Whether the gmail's browser's last check went through.
const BROWSER: &str = "EXISTS (SELECT 1 FROM gmail_browsers w WHERE w.gmail_id = gmails.id AND w.check_error IS NULL)";

type GmailRow = (i64, Uuid, String, Option<String>, i64, bool);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gmail {
	pub id: i64,
	pub owner: Uuid,
	pub gmail: String,
	/// `None` until its owner says.
	pub role: Option<GmailRole>,
	pub created_at: Timestamp,
	/// Its browser set up, its last check through.
	pub browser: bool,
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
	/// Its category is this trade in its country.
	pub trade_id: Option<i64>,
	/// Its service areas, none to 20.
	pub areas: Vec<String>,
	/// Where the verification film is shot.
	pub hidden_address: Option<String>,
	/// `false`: a service-area business, its address hidden.
	pub customers_visit: Option<bool>,
	/// verif_vid's film, recorded by hand.
	pub film: Option<String>,
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

/// What a new GBP row holds; `gmail_id` absent for a draft, `place_id` for one not on Maps.
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
	pub at: Option<LatLng>,
	/// Placed by hand, not Google's.
	pub at_by_hand: bool,
	/// The owner's.
	pub trade_id: Option<i64>,
}

/// The owner's gmails not removed, by address.
pub async fn gmails(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<Gmail>> {
	let rows: Vec<GmailRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"SELECT id, owner, gmail, role, created_at, {BROWSER} FROM gmails WHERE owner = $1 AND removed_at IS NULL ORDER BY gmail"
	)))
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
		 ON CONFLICT DO NOTHING RETURNING id, owner, gmail, role, created_at, FALSE",
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

/// The gmail removed, and the GBPs it manages with it, at one `now` (a restore brings back
/// those). `false`: no such gmail of the owner's, or
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

/// The owner's gmail `id` in `role`, or in none said. `false`: no such gmail of theirs, or it is removed.
pub async fn set_role(conn: &mut SqliteConnection, owner: Uuid, id: i64, role: Option<GmailRole>) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE gmails SET role = $3 WHERE id = $1 AND owner = $2 AND removed_at IS NULL")
		.bind(id)
		.bind(owner)
		.bind(role.map(GmailRole::as_str))
		.execute(&mut *conn)
		.await
		.wrap_err("setting a gmail's role")?;
	Ok(done.rows_affected() == 1)
}

/// The owner of a gmail not removed.
pub async fn gmail_owner(conn: &mut SqliteConnection, id: i64) -> eyre::Result<Option<Uuid>> {
	sqlx::query_scalar("SELECT owner FROM gmails WHERE id = $1 AND removed_at IS NULL")
		.bind(id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a gmail")
}

/// What a GBP holds that is one live GBP's; each absent is not looked for.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unique<'a> {
	pub name: Option<&'a str>,
	pub place_id: Option<&'a str>,
	/// In [`sa_core::gbp::phone`]'s form.
	pub phone: Option<&'a str>,
}

/// Why `u` cannot be GBP `except`'s: another live GBP has some of it.
pub async fn taken(conn: &mut SqliteConnection, u: Unique<'_>, except: Option<i64>) -> eyre::Result<Option<String>> {
	let (by_name, by_place, by_phone): (Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
		"SELECT (SELECT id FROM gmbs WHERE name = $1 AND removed_at IS NULL AND id IS NOT $4), \
		 (SELECT id FROM gmbs WHERE place_id = $2 AND removed_at IS NULL AND id IS NOT $4), \
		 (SELECT id FROM gmbs WHERE phone = $3 AND removed_at IS NULL AND id IS NOT $4)",
	)
	.bind(u.name)
	.bind(u.place_id)
	.bind(u.phone)
	.bind(except)
	.fetch_one(&mut *conn)
	.await
	.wrap_err("looking for a GMB's name, place and phone")?;
	Ok(match (by_name, by_place, by_phone) {
		(Some(id), _, _) => Some(format!("GBP {id} is named {:?} already", u.name.expect("matched by it"))),
		(None, Some(id), _) => Some(format!("GBP {id} is on {} already", u.place_id.expect("matched by it"))),
		(None, None, Some(id)) => Some(format!("GBP {id} has {} already", u.phone.expect("matched by it"))),
		(None, None, None) => None,
	})
}

/// `id` explicit only on import. Its name and place are free ([`taken`]).
pub async fn add_gmb(conn: &mut SqliteConnection, id: Option<i64>, new: &NewGmb<'_>, now: Timestamp) -> eyre::Result<Gmb> {
	let added: i64 = sqlx::query_scalar(
		"INSERT INTO gmbs (id, owner, gmail_id, place_id, name, address, state, country, country_by, lat, lng, placed_by, created_at, trade_id) \
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, CASE WHEN $9 THEN $2 END, $10, $11, CASE WHEN $12 THEN $2 END, $13, $14) RETURNING id",
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
	.bind(new.at_by_hand)
	.bind(to_db(now))
	.bind(new.trade_id)
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

/// `e`'s fields but the name and the gmail, which move by their own rules; the phone free
/// ([`taken`]), the trade the owner's.
pub async fn set_fields(conn: &mut SqliteConnection, id: i64, e: &crate::gmbs::Edit) -> eyre::Result<()> {
	let areas = e.areas.as_ref().map(|a| a.as_ref().map(|a| serde_json::to_string(a).expect("strings serialize")));
	sqlx::query(
		"UPDATE gmbs SET phone = CASE WHEN $2 THEN $3 ELSE phone END, trade_id = CASE WHEN $4 THEN $5 ELSE trade_id END, \
		 areas = CASE WHEN $6 THEN $7 ELSE areas END, hidden_address = CASE WHEN $8 THEN $9 ELSE hidden_address END, \
		 customers_visit = CASE WHEN $10 THEN $11 ELSE customers_visit END, film = CASE WHEN $12 THEN $13 ELSE film END WHERE id = $1",
	)
	.bind(id)
	.bind(e.phone.is_some())
	.bind(e.phone.clone().flatten())
	.bind(e.trade.is_some())
	.bind(e.trade.flatten())
	.bind(areas.is_some())
	.bind(areas.flatten())
	.bind(e.hidden_address.is_some())
	.bind(e.hidden_address.clone().flatten())
	.bind(e.customers_visit.is_some())
	.bind(e.customers_visit.flatten())
	.bind(e.film.is_some())
	.bind(e.film.clone().flatten())
	.execute(&mut *conn)
	.await
	.wrap_err("editing a GMB")?;
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

/// GBP `id` in `to` with its place: Google's address, and Google's point and country over those
/// set by hand.
pub async fn place_on_google(conn: &mut SqliteConnection, id: i64, place_id: &str, address: Option<&str>, at: Option<LatLng>, country: Option<Country>, to: State) -> eyre::Result<()> {
	// country first: the schema refuses a placed GBP without one
	if let Some(c) = country {
		set_country(conn, id, c).await?;
	}
	if let Some(at) = at {
		set_point(conn, id, at).await?;
	}
	sqlx::query("UPDATE gmbs SET place_id = $2, address = $3, state = $4 WHERE id = $1")
		.bind(id)
		.bind(place_id)
		.bind(address)
		.bind(to.as_str())
		.execute(&mut *conn)
		.await
		.wrap_err("placing a GBP on Google")?;
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

/// The point the owner placed on GBP `gmb_id` taken back. `Err(NotFound)`: not their GBP, or
/// removed; `Err(Conflict)`: it has no point placed by hand.
pub async fn unplace(conn: &mut SqliteConnection, owner: Uuid, gmb_id: i64) -> eyre::Result<Result<(), crate::Refused>> {
	let Some(gbp) = gmb_of(conn, Some(owner), gmb_id).await? else {
		return Ok(Err(crate::Refused::NotFound));
	};
	if !gbp.location.is_some_and(|p| p.placed) {
		return Ok(Err(crate::Refused::Conflict(format!("GBP {gmb_id} has no point placed by hand"))));
	}
	sqlx::query("UPDATE gmbs SET lat = NULL, lng = NULL, placed_by = NULL WHERE id = $1")
		.bind(gmb_id)
		.execute(&mut *conn)
		.await
		.wrap_err("taking a placed point back")?;
	Ok(Ok(()))
}

/// The live GBPs on Google without a point or a country of Google's: each id, its place, and
/// whether it has no point.
pub async fn unlocated(conn: &mut SqliteConnection) -> eyre::Result<Vec<(i64, String, bool, Uuid, i64)>> {
	sqlx::query_as(
		"SELECT id, place_id, lat IS NULL, owner, gmail_id FROM gmbs \
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

/// A trade of its owner's: Plumbing, set up in some countries.
#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
	pub id: i64,
	pub name: String,
	pub created_at: Timestamp,
	/// By country.
	pub categories: Vec<Category>,
}

/// A trade set up for a country: Plumbing · FR.
#[derive(Clone, Debug, PartialEq)]
pub struct Category {
	pub country: Country,
	/// In [`sa_core::gbp::keywords`]' form.
	pub keywords: Vec<String>,
	/// Google's category it is typed into Google's form as.
	pub google_primary: Option<GoogleCategory>,
	/// Google's additional categories, in the order they are entered.
	pub google_additional: Vec<GoogleCategory>,
	/// Placement's trade that studies it.
	pub study: Option<String>,
}

/// The owner's trades, by name.
pub async fn trades(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<Trade>> {
	let rows: Vec<(i64, String, i64)> = sqlx::query_as("SELECT id, name, created_at FROM trades WHERE owner = $1 ORDER BY lower(name), id")
		.bind(owner)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing trades")?;
	let categories: Vec<CategoryRow> = sqlx::query_as(
		"SELECT c.trade_id, c.country, c.keywords, c.google_primary, c.google_additional, c.study FROM categories c JOIN trades t ON t.id = c.trade_id \
		 WHERE t.owner = $1 ORDER BY c.country",
	)
	.bind(owner)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing categories")?;
	let mut trades = rows
		.into_iter()
		.map(|(id, name, created_at)| {
			Ok(Trade {
				id,
				name,
				created_at: from_db(created_at)?,
				categories: vec![],
			})
		})
		.collect::<eyre::Result<Vec<_>>>()?;
	for CategoryRow {
		trade_id,
		country,
		keywords,
		google_primary,
		google_additional,
		study,
	} in categories
	{
		let trade = trades.iter_mut().find(|t| t.id == trade_id).expect("joined on the owner's trades");
		let c: Country = country.parse()?;
		// A gcid a refresh of the list dropped: its rows need migrating to what Google replaced it with.
		let google = |id: &str| GoogleCategory::find(c, id).wrap_err_with(|| format!("trade {trade_id}'s Google category in {country}"));
		let additional: Vec<String> = serde_json::from_str(&google_additional).wrap_err_with(|| format!("trade {trade_id}'s additional Google categories in {country}"))?;
		trade.categories.push(Category {
			country: c,
			keywords: serde_json::from_str(&keywords).wrap_err_with(|| format!("trade {trade_id}'s keywords in {country}"))?,
			google_primary: google_primary.as_deref().map(google).transpose()?,
			google_additional: additional.iter().map(|id| google(id)).collect::<eyre::Result<_>>()?,
			study,
		});
	}
	Ok(trades)
}

/// The owner of trade `id`.
pub async fn trade_owner(conn: &mut SqliteConnection, id: i64) -> eyre::Result<Option<Uuid>> {
	sqlx::query_scalar("SELECT owner FROM trades WHERE id = $1")
		.bind(id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a trade")
}

/// The owner's trade named `name` whatever its case, but `except`.
pub async fn trade_named(conn: &mut SqliteConnection, owner: Uuid, name: &str, except: Option<i64>) -> eyre::Result<Option<i64>> {
	let names: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM trades WHERE owner = $1 AND id IS NOT $2")
		.bind(owner)
		.bind(except)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing trades' names")?;
	let folded = name.to_lowercase();
	Ok(names.into_iter().find(|(_, n)| n.to_lowercase() == folded).map(|(id, _)| id))
}

/// Its name free ([`trade_named`]).
pub async fn add_trade(conn: &mut SqliteConnection, owner: Uuid, name: &str, now: Timestamp) -> eyre::Result<i64> {
	sqlx::query_scalar("INSERT INTO trades (owner, name, created_at) VALUES ($1, $2, $3) RETURNING id")
		.bind(owner)
		.bind(name)
		.bind(to_db(now))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("adding a trade")
}

/// Its name free ([`trade_named`]).
pub async fn rename_trade(conn: &mut SqliteConnection, id: i64, name: &str) -> eyre::Result<()> {
	sqlx::query("UPDATE trades SET name = $2 WHERE id = $1")
		.bind(id)
		.bind(name)
		.execute(&mut *conn)
		.await
		.wrap_err("renaming a trade")?;
	Ok(())
}

/// How many live GBPs hold trade `id`.
pub async fn trade_holders(conn: &mut SqliteConnection, id: i64) -> eyre::Result<i64> {
	sqlx::query_scalar("SELECT COUNT(*) FROM gmbs WHERE trade_id = $1 AND removed_at IS NULL")
		.bind(id)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("counting a trade's GMBs")
}

/// Trade `id` and its categories, the removed GBPs giving it up; the schema refuses it while a live
/// one holds it.
pub async fn delete_trade(conn: &mut SqliteConnection, id: i64) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET trade_id = NULL WHERE trade_id = $1 AND removed_at IS NOT NULL")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("giving a trade up")?;
	sqlx::query("DELETE FROM trades WHERE id = $1").bind(id).execute(&mut *conn).await.wrap_err("deleting a trade")?;
	Ok(())
}

/// Trade `id` set up for `c.country` as `c` says, over what it was.
pub async fn set_up(conn: &mut SqliteConnection, id: i64, c: &Category) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO categories (trade_id, country, keywords, google_primary, google_additional, study) VALUES ($1, $2, $3, $4, $5, $6) \
		 ON CONFLICT (trade_id, country) DO UPDATE SET keywords = excluded.keywords, google_primary = excluded.google_primary, \
		 google_additional = excluded.google_additional, study = excluded.study",
	)
	.bind(id)
	.bind(c.country.as_str())
	.bind(serde_json::to_string(&c.keywords).expect("strings serialize"))
	.bind(c.google_primary.map(|g| g.id))
	.bind(serde_json::to_string(&c.google_additional.iter().map(|g| g.id).collect::<Vec<_>>()).expect("strings serialize"))
	.bind(&c.study)
	.execute(&mut *conn)
	.await
	.wrap_err("setting a category up")?;
	Ok(())
}

/// `false`: trade `id` is not set up there.
pub async fn remove_category(conn: &mut SqliteConnection, id: i64, country: Country) -> eyre::Result<bool> {
	let done = sqlx::query("DELETE FROM categories WHERE trade_id = $1 AND country = $2")
		.bind(id)
		.bind(country.as_str())
		.execute(&mut *conn)
		.await
		.wrap_err("removing a category")?;
	Ok(done.rows_affected() == 1)
}

/// Each of `ids` given `trade`, the owner's. `false`: one is no GBP of the owner's not removed,
/// and the others may be written: roll back.
pub async fn set_trades(conn: &mut SqliteConnection, owner: Uuid, ids: &BTreeSet<i64>, trade: Option<i64>) -> eyre::Result<bool> {
	let asked = i64::try_from(ids.len()).expect("a few ids");
	let ids = serde_json::to_string(ids).expect("ids serialize");
	let done = sqlx::query("UPDATE gmbs SET trade_id = $3 WHERE id IN (SELECT value FROM json_each($1)) AND owner = $2 AND removed_at IS NULL")
		.bind(&ids)
		.bind(owner)
		.bind(trade)
		.execute(&mut *conn)
		.await
		.wrap_err("setting GMBs' trade")?;
	Ok(i64::try_from(done.rows_affected()).expect("a few rows") == asked)
}

#[derive(sqlx::FromRow)]
struct CategoryRow {
	trade_id: i64,
	country: String,
	keywords: String,
	google_primary: Option<String>,
	google_additional: String,
	study: Option<String>,
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
	trade_id: Option<i64>,
	areas: Option<String>,
	hidden_address: Option<String>,
	customers_visit: Option<bool>,
	film: Option<String>,
	lat: Option<f64>,
	lng: Option<f64>,
	placed: bool,
	created_at: i64,
	enabled: bool,
}

fn gmail((id, owner, gmail, role, created_at, browser): GmailRow) -> eyre::Result<Gmail> {
	Ok(Gmail {
		id,
		owner,
		gmail,
		role: role.as_deref().map(str::parse).transpose()?,
		created_at: from_db(created_at)?,
		browser,
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
		trade_id: r.trade_id,
		areas: r
			.areas
			.as_deref()
			.map(serde_json::from_str)
			.transpose()
			.wrap_err_with(|| format!("GMB {}'s areas", r.id))?
			.unwrap_or_default(),
		hidden_address: r.hidden_address,
		customers_visit: r.customers_visit,
		film: r.film,
		created_at: from_db(r.created_at)?,
		enabled: r.enabled,
		location,
	})
}
