//! People's gmails and the GBPs each manages. Never deleted: a removed row keeps its id, which
//! the services hold.

use std::collections::{BTreeSet, HashMap};

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::gbp::{Country, Managed, Role, State};
use sa_places::LatLng;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{connections::Checked, from_db, to_db};
use crate::mail::SentBy;

const GMB: &str = "SELECT b.id, b.owner, b.gmail_id, b.place_id, b.name, b.address, b.state, b.country, b.country_by IS NOT NULL AS country_by_hand, b.phone, \
	b.trade, b.brand_id, b.areas, b.hidden_address, b.customers_visit, b.film, b.role, b.owner_gmail, b.lat, b.lng, b.placed_by IS NOT NULL AS placed, b.created_at, b.website, b.hours, b.google_primary, b.synced_at, \
	(SELECT json_group_array(json_object('id', p.id, 'author', p.author)) FROM (SELECT * FROM gmb_photos p WHERE p.gmb_id = b.id ORDER BY p.n) p) AS photos, NOT EXISTS (SELECT 1 FROM gmbs_off o WHERE o.gmb_id = b.id) AS enabled, \
	(SELECT json_group_array(json_object('lat', s.lat, 'lng', s.lng, 'label', s.label, 'note', s.note)) FROM (SELECT * FROM gmb_spots s WHERE s.gmb_id = b.id ORDER BY s.pos) s) AS spots FROM gmbs b";

/// Whether the gmail's browser's last check went through.
const BROWSER: &str = "EXISTS (SELECT 1 FROM gmail_browsers w WHERE w.gmail_id = gmails.id AND w.check_error IS NULL)";

/// Of which GBP a message kept is, by its gmail and Gmail's id of it, and who sent it.
pub type Filed = HashMap<(i64, String), (Option<i64>, Option<SentBy>)>;

type GmailRow = (i64, Uuid, String, i64, bool, Option<i64>, Option<String>);

/// A GBP, by its id.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GmbId {
	pub v: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gmail {
	pub id: i64,
	pub owner: Uuid,
	pub gmail: String,
	pub created_at: Timestamp,
	/// Its browser set up, its last check through.
	pub browser: bool,
	/// Its inbox's last check; `None`: none set.
	pub imap: Option<Checked>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gmb {
	pub id: i64,
	pub owner: Uuid,
	/// `None`: a draft no gmail manages yet.
	pub gmail_id: Option<i64>,
	/// `None` while a draft, and only then.
	pub place_id: Option<String>,
	pub name: String,
	pub address: Option<String>,
	pub state: State,
	pub country: Option<GbpCountry>,
	pub phone: Option<String>,
	/// The gcid it is meant to have as its primary: its category is its owner's of it in its country.
	pub trade: Option<String>,
	/// Of its gmail and its category.
	pub brand_id: Option<i64>,
	/// Its service areas, none to 20.
	pub areas: Vec<String>,
	/// Where the verification film is shot.
	pub hidden_address: Option<String>,
	/// `false`: a service-area business, its address hidden.
	pub customers_visit: Option<bool>,
	/// verif_vid's film, recorded by hand.
	pub film: Option<String>,
	/// Its gmail's on it; `None` until said, and without a gmail.
	pub role: Option<Role>,
	/// A manager's: who owns it, for reference.
	pub owner_gmail: Option<String>,
	pub created_at: Timestamp,
	/// Its owner's switch.
	pub enabled: bool,
	/// `None`: none on Google (a service-area place, a draft) and none placed yet, or not located
	/// since it was added without a Places key.
	pub location: Option<Point>,
	/// A draft's candidate spots, its point the first; none past a draft.
	pub spots: Vec<Spot>,
	/// Google's, from the last Sync, as each below.
	pub website: Option<String>,
	/// A line a weekday, Monday first.
	pub hours: Vec<String>,
	/// `gcid:plumber`: Google's primary category, when the catalog knows it.
	pub google_primary: Option<String>,
	/// In Google's order.
	pub photos: Vec<GmbPhoto>,
	/// When a Sync last wrote it.
	pub synced_at: Option<Timestamp>,
}

/// A photo a Sync took of a GBP's profile on Google.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, schemars::JsonSchema, serde::Serialize)]
pub struct GmbPhoto {
	/// Never another photo's.
	pub id: i64,
	/// Google's attribution.
	pub author: Option<String>,
}

/// What a Sync writes over a GBP: Google's, each as it answered.
pub struct Synced<'a> {
	pub name: &'a str,
	pub address: Option<&'a str>,
	/// In [`sa_core::gbp::phone`]'s form, free ([`taken`]).
	pub phone: Option<&'a str>,
	pub website: Option<&'a str>,
	pub hours: &'a [String],
	pub google_primary: Option<&'a str>,
	/// Each photo's bytes and author.
	pub photos: &'a [(Vec<u8>, Option<String>)],
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, schemars::JsonSchema, serde::Serialize)]
pub struct Spot {
	#[serde(flatten)]
	pub at: LatLng,
	/// What it was picked by: an address, a business nearby.
	pub label: Option<String>,
	pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, schemars::JsonSchema, serde::Serialize)]
pub struct Point {
	#[serde(flatten)]
	pub at: LatLng,
	/// Placed by hand on the map, Google having none.
	pub placed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, schemars::JsonSchema, serde::Serialize)]
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
	/// A gcid Google lists.
	pub trade: Option<&'a str>,
}

/// What a GBP's log holds, a row each (`gmb_events`).
#[derive(Clone, Debug, PartialEq, serde::Deserialize, strum::EnumDiscriminants, schemars::JsonSchema, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum_discriminants(
	name(EventKind),
	derive(serde::Deserialize, schemars::JsonSchema, serde::Serialize, strum::EnumString, strum::IntoStaticStr, strum::VariantArray),
	serde(rename_all = "snake_case"),
	strum(serialize_all = "snake_case")
)]
pub enum Event {
	/// Created.
	Added {
		state: State,
	},
	State {
		from: State,
		to: State,
	},
	/// By hand.
	Edited {
		changes: Vec<Changed>,
	},
	/// Overwritten with its profile on Google; one that changed nothing too.
	Synced {
		changes: Vec<Changed>,
	},
	Gmail {
		from: Option<i64>,
		to: Option<i64>,
	},
	Trade {
		from: Option<Traded>,
		to: Option<Traded>,
	},
	Brand {
		from: Option<i64>,
		to: Option<i64>,
	},
	/// verif_vid's film, set or cleared.
	Film {
		film: Option<String>,
	},
	/// Its owner's switch.
	Switched {
		on: bool,
	},
	Removed,
	Restored,
	/// Its gmail's browser launched: at its point, or for the gmail alone (then each of its GBPs').
	Launched {
		/// The launch plays a film as the camera.
		film: bool,
	},
	/// Its gmail's browser stopped by its owner.
	Stopped,
	/// A message with Google in its gmail's inbox naming it; one naming none is the gmail's.
	Email {
		/// Gmail's X-GM-MSGID.
		message: String,
		/// Gmail's X-GM-THRID, hex: the thread's address in Gmail.
		thread: String,
		/// When Gmail took it in.
		date: Timestamp,
		subject: Option<String>,
		/// Its sender; one sent, its recipients.
		counterparts: Vec<String>,
		snippet: String,
		/// From the gmail.
		sent: bool,
	},
}

/// A trade as its log names it.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, schemars::JsonSchema, serde::Serialize)]
#[serde(untagged)]
pub enum Traded {
	Gcid(String),
	/// A trade a person named, by its retired id: the log keeps what it was told.
	Retired(i64),
}

/// A field of a GBP, before and after.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, schemars::JsonSchema, serde::Serialize)]
pub struct Changed {
	pub field: Field,
	pub from: serde_json::Value,
	pub to: serde_json::Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, schemars::JsonSchema, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
	Name,
	PlaceId,
	Address,
	Phone,
	Website,
	Hours,
	GooglePrimary,
	Country,
	Point,
	Photos,
	Areas,
	HiddenAddress,
	CustomersVisit,
	Spots,
	Role,
	OwnerGmail,
}

/// `event` in the log of GBP `gmb` (none: of gmail `gmail`'s every GBP), `gmail` the one it was under.
pub async fn log(conn: &mut SqliteConnection, gmb: Option<i64>, gmail: Option<i64>, by: Option<Uuid>, now: Timestamp, event: &Event) -> eyre::Result<()> {
	let kind: &'static str = EventKind::from(event).into();
	sqlx::query("INSERT INTO gmb_events (gmb_id, gmail_id, at, by, kind, data) VALUES ($1, $2, $3, $4, $5, $6)")
		.bind(gmb)
		.bind(gmail)
		.bind(to_db(now))
		.bind(by)
		.bind(kind)
		.bind(serde_json::to_string(event).expect("an event serializes"))
		.execute(&mut *conn)
		.await
		.wrap_err("logging a GBP's event")?;
	Ok(())
}

/// What `before` → `after` changed of a GBP, in its log; a Sync's is logged changing nothing too.
pub async fn record(conn: &mut SqliteConnection, by: Option<Uuid>, now: Timestamp, before: &Gmb, after: &Gmb, synced: bool) -> eyre::Result<()> {
	for e in diff(before, after, synced) {
		log(conn, Some(after.id), before.gmail_id, by, now, &e).await?;
	}
	Ok(())
}

/// What changed of each of `befores` since it was read, in its log.
pub async fn record_since(conn: &mut SqliteConnection, by: Option<Uuid>, now: Timestamp, befores: &[Gmb]) -> eyre::Result<()> {
	for before in befores {
		let after = gmb_of(conn, None, before.id).await?.expect("live through its own write");
		record(conn, by, now, before, &after, false).await?;
	}
	Ok(())
}

/// The live GBPs of `ids`, as they are now.
pub async fn of_ids(conn: &mut SqliteConnection, ids: &BTreeSet<i64>) -> eyre::Result<Vec<Gmb>> {
	let rows: Vec<GmbRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"{GMB} WHERE b.id IN (SELECT value FROM json_each($1)) AND b.removed_at IS NULL ORDER BY b.id"
	)))
	.bind(serde_json::to_string(ids).expect("ids serialize"))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("loading GBPs")?;
	rows.into_iter().map(gmb).collect()
}

/// A row of a GBP's log.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
	pub id: i64,
	pub at: Timestamp,
	/// Who did it, by name; none for Google or a run.
	pub by: Option<String>,
	/// The gmail it was under.
	pub gmail_id: Option<i64>,
	/// Its gmail's, not its own.
	pub of_gmail: bool,
	/// An email of ours: who sent it.
	pub sent_by: Option<SentBy>,
	pub event: Event,
}

/// A page of GBP `gmb`'s log before event `before`, newest first, of `kinds` (none: every kind); each of its
/// gmails' own while it was under it; an email `filed` with a GBP is that one's. `None`: no live GBP of the owner's.
pub async fn events(conn: &mut SqliteConnection, owner: Uuid, gmb: i64, before: Option<i64>, kinds: &[EventKind], filed: &Filed, limit: u32) -> eyre::Result<Option<Vec<Logged>>> {
	let Some(b) = gmb_of(conn, Some(owner), gmb).await? else {
		return Ok(None);
	};
	let kinds: Vec<&'static str> = kinds.iter().map(|k| k.into()).collect();
	// its own, and the other GBPs'
	let keys = |own: bool| {
		let of = filed.iter().filter(|(_, (g, _))| g.is_some_and(|g| (g == gmb) == own));
		let keys: Vec<String> = of.map(|((gmail, message), _)| format!("{gmail}:{message}")).collect();
		serde_json::to_string(&keys).expect("keys serialize")
	};
	type Row = (i64, i64, Option<String>, Option<i64>, bool, String);
	let rows: Vec<Row> = sqlx::query_as(
		"SELECT e.id, e.at, u.name, e.gmail_id, e.gmb_id IS NULL, e.data FROM gmb_events e LEFT JOIN users u ON u.user_id = e.by \
		 WHERE CASE WHEN e.kind = 'email' AND e.gmail_id || ':' || (e.data ->> 'message') IN (SELECT value FROM json_each($7)) THEN \
		     e.id = (SELECT min(o.id) FROM gmb_events o WHERE o.kind = 'email' AND o.gmail_id = e.gmail_id AND o.data ->> 'message' = e.data ->> 'message') \
		   WHEN e.kind = 'email' AND e.gmail_id || ':' || (e.data ->> 'message') IN (SELECT value FROM json_each($8)) THEN 0 \
		   ELSE e.gmb_id = $1 OR (e.gmb_id IS NULL AND e.at >= $3 AND e.gmail_id IS \
		     coalesce((SELECT o.gmail_id FROM gmb_events o WHERE o.gmb_id = $1 AND o.id > e.id ORDER BY o.id LIMIT 1), $2)) END \
		 AND ($4 IS NULL OR e.id < $4) AND ($5 = '[]' OR e.kind IN (SELECT value FROM json_each($5))) \
		 ORDER BY e.id DESC LIMIT $6",
	)
	.bind(gmb)
	.bind(b.gmail_id)
	.bind(to_db(b.created_at))
	.bind(before)
	.bind(serde_json::to_string(&kinds).expect("kinds serialize"))
	.bind(limit)
	.bind(keys(true))
	.bind(keys(false))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("reading a GBP's log")?;
	let logged = rows
		.into_iter()
		.map(|(id, at, by, gmail_id, of_gmail, data)| {
			let event: Event = serde_json::from_str(&data).wrap_err_with(|| format!("GBP event {id}"))?;
			let mail = match &event {
				Event::Email { message, .. } => gmail_id.and_then(|g| filed.get(&(g, message.clone()))),
				_ => None,
			};
			Ok(Logged {
				id,
				at: from_db(at)?,
				by,
				gmail_id,
				of_gmail: of_gmail && mail.is_none_or(|(g, _)| g.is_none()),
				sent_by: mail.and_then(|(_, by)| by.clone()),
				event,
			})
		})
		.collect::<eyre::Result<_>>()?;
	Ok(Some(logged))
}

/// The owner's gmails not removed, by address.
pub async fn gmails(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<Gmail>> {
	let rows: Vec<GmailRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"SELECT id, owner, gmail, created_at, {BROWSER}, i.checked_at, i.check_error FROM gmails LEFT JOIN gmail_imap i ON i.gmail_id = gmails.id WHERE owner = $1 AND removed_at IS NULL ORDER BY gmail"
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
		 ON CONFLICT DO NOTHING RETURNING id, owner, gmail, created_at, FALSE, NULL, NULL",
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
		"INSERT INTO gmbs (id, owner, gmail_id, place_id, name, address, state, country, country_by, lat, lng, placed_by, created_at, trade) \
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
	.bind(new.trade)
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

/// To another gmail of its owner's: the schema refuses anyone else's. Its brand and its role stay its old gmail's.
pub async fn move_to(conn: &mut SqliteConnection, id: GmbId, gmail_id: i64) -> eyre::Result<()> {
	sqlx::query(
		"UPDATE gmbs SET gmail_id = $2, brand_id = CASE WHEN gmail_id IS $2 THEN brand_id END, role = CASE WHEN gmail_id IS $2 THEN role END, \
		 owner_gmail = CASE WHEN gmail_id IS $2 THEN owner_gmail END WHERE id = $1",
	)
	.bind(id.v)
	.bind(gmail_id)
	.execute(&mut *conn)
	.await
	.wrap_err("moving a GMB")?;
	Ok(())
}

/// `e`'s fields but the name and the gmail, which move by their own rules; the phone free
/// ([`taken`]), another trade dropping the brand, a role but manager the owner gmail.
pub async fn set_fields(conn: &mut SqliteConnection, id: i64, e: &crate::gmbs::Edit) -> eyre::Result<()> {
	let areas = e.areas.as_ref().map(|a| a.as_ref().map(|a| serde_json::to_string(a).expect("strings serialize")));
	sqlx::query(
		"UPDATE gmbs SET phone = CASE WHEN $2 THEN $3 ELSE phone END, trade = CASE WHEN $4 THEN $5 ELSE trade END, \
		 brand_id = CASE WHEN $4 AND trade IS NOT $5 THEN NULL ELSE brand_id END, \
		 areas = CASE WHEN $6 THEN $7 ELSE areas END, hidden_address = CASE WHEN $8 THEN $9 ELSE hidden_address END, \
		 customers_visit = CASE WHEN $10 THEN $11 ELSE customers_visit END, film = CASE WHEN $12 THEN $13 ELSE film END, \
		 role = CASE WHEN $14 THEN $15 ELSE role END, \
		 owner_gmail = CASE WHEN $16 THEN $17 WHEN $14 AND $15 IS NOT 'manager' THEN NULL ELSE owner_gmail END WHERE id = $1",
	)
	.bind(id)
	.bind(e.phone.is_some())
	.bind(e.phone.clone().flatten())
	.bind(e.trade.is_some())
	.bind(e.trade.clone().flatten())
	.bind(areas.is_some())
	.bind(areas.flatten())
	.bind(e.hidden_address.is_some())
	.bind(e.hidden_address.clone().flatten())
	.bind(e.customers_visit.is_some())
	.bind(e.customers_visit.flatten())
	.bind(e.film.is_some())
	.bind(e.film.clone().flatten())
	.bind(e.role.is_some())
	.bind(e.role.flatten().map(Role::as_str))
	.bind(e.owner_gmail.is_some())
	.bind(e.owner_gmail.clone().flatten())
	.execute(&mut *conn)
	.await
	.wrap_err("editing a GMB")?;
	Ok(())
}

/// `user`'s own note on GBP `gmb`.
pub async fn note(conn: &mut SqliteConnection, user: Uuid, gmb: i64) -> eyre::Result<Option<String>> {
	sqlx::query_scalar("SELECT note FROM gmb_notes WHERE user_id = $1 AND gmb_id = $2")
		.bind(user)
		.bind(gmb)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("reading a note on a GBP")
}

/// `user`'s own note on GBP `gmb`; `None` takes it away.
pub async fn set_note(conn: &mut SqliteConnection, user: Uuid, gmb: i64, note: Option<&str>) -> eyre::Result<()> {
	let write = match note {
		None => sqlx::query("DELETE FROM gmb_notes WHERE user_id = $1 AND gmb_id = $2").bind(user).bind(gmb),
		Some(note) => sqlx::query("INSERT INTO gmb_notes (user_id, gmb_id, note) VALUES ($1, $2, $3) ON CONFLICT (user_id, gmb_id) DO UPDATE SET note = excluded.note")
			.bind(user)
			.bind(gmb)
			.bind(note),
	};
	write.execute(&mut *conn).await.wrap_err("writing a note on a GBP")?;
	Ok(())
}

/// GBP `id` in `to`; the schema checks the move against [`State::next`] again. Out of a draft,
/// its first spot stays its point, the others go.
pub async fn set_state(conn: &mut SqliteConnection, id: i64, to: State) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET state = $2 WHERE id = $1")
		.bind(id)
		.bind(to.as_str())
		.execute(&mut *conn)
		.await
		.wrap_err("moving a GMB's state")?;
	sqlx::query("DELETE FROM gmb_spots WHERE gmb_id = $1")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("dropping a draft's spots")?;
	Ok(())
}

/// Draft `id`'s spots, in order, `by` its owner: the first its point, placed by hand.
pub async fn set_spots(conn: &mut SqliteConnection, by: Uuid, id: i64, spots: &[Spot]) -> eyre::Result<()> {
	sqlx::query("DELETE FROM gmb_spots WHERE gmb_id = $1")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("clearing a draft's spots")?;
	for (pos, s) in spots.iter().enumerate() {
		sqlx::query("INSERT INTO gmb_spots (gmb_id, pos, lat, lng, label, note) VALUES ($1, $2, $3, $4, $5, $6)")
			.bind(id)
			.bind(i64::try_from(pos).expect("a few spots"))
			.bind(s.at.lat)
			.bind(s.at.lng)
			.bind(s.label.as_deref())
			.bind(s.note.as_deref())
			.execute(&mut *conn)
			.await
			.wrap_err("adding a draft's spot")?;
	}
	let first = spots.first().map(|s| s.at);
	sqlx::query("UPDATE gmbs SET lat = $2, lng = $3, placed_by = CASE WHEN $2 IS NULL THEN NULL ELSE $4 END WHERE id = $1")
		.bind(id)
		.bind(first.map(|a| a.lat))
		.bind(first.map(|a| a.lng))
		.bind(by)
		.execute(&mut *conn)
		.await
		.wrap_err("placing a draft at its first spot")?;
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
	sqlx::query("DELETE FROM gmb_spots WHERE gmb_id = $1")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("dropping a draft's spots")?;
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
	if gbp.state == State::Draft {
		let mut spots = gbp.spots;
		match spots.first_mut() {
			Some(first) => first.at = at,
			None => spots.push(Spot { at, label: None, note: None }),
		}
		set_spots(conn, owner, gmb_id, &spots).await?;
		return Ok(Ok(()));
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
	if gbp.state == State::Draft {
		set_spots(conn, owner, gmb_id, &gbp.spots[1..]).await?;
		return Ok(Ok(()));
	}
	sqlx::query("UPDATE gmbs SET lat = NULL, lng = NULL, placed_by = NULL WHERE id = $1")
		.bind(gmb_id)
		.execute(&mut *conn)
		.await
		.wrap_err("taking a placed point back")?;
	Ok(Ok(()))
}

/// GBP `id`'s profile as `s` says, its photos replaced; with no trade, it takes Google's primary.
pub async fn sync(conn: &mut SqliteConnection, id: i64, s: &Synced<'_>, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET name = $2, address = $3, phone = $4, website = $5, hours = $6, google_primary = $7, synced_at = $8 WHERE id = $1")
		.bind(id)
		.bind(s.name)
		.bind(s.address)
		.bind(s.phone)
		.bind(s.website)
		.bind((!s.hours.is_empty()).then(|| serde_json::to_string(s.hours).expect("strings serialize")))
		.bind(s.google_primary)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("syncing a GBP")?;
	sqlx::query("UPDATE gmbs SET trade = google_primary WHERE id = $1 AND trade IS NULL AND google_primary IS NOT NULL")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("giving a synced GBP its trade")?;
	sqlx::query("DELETE FROM gmb_photos WHERE gmb_id = $1")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("dropping a GBP's photos")?;
	for (n, (bytes, author)) in s.photos.iter().enumerate() {
		sqlx::query("INSERT INTO gmb_photos (gmb_id, n, bytes, author) VALUES ($1, $2, $3, $4)")
			.bind(id)
			.bind(i64::try_from(n).expect("a few photos"))
			.bind(bytes)
			.bind(author.as_deref())
			.execute(&mut *conn)
			.await
			.wrap_err("adding a GBP's photo")?;
	}
	Ok(())
}

/// Photo `photo` of the owner's GBP `id`, not removed.
pub async fn photo(conn: &mut SqliteConnection, owner: Uuid, id: i64, photo: i64) -> eyre::Result<Option<Vec<u8>>> {
	sqlx::query_scalar("SELECT p.bytes FROM gmb_photos p JOIN gmbs b ON b.id = p.gmb_id WHERE p.gmb_id = $1 AND p.id = $2 AND b.owner = $3 AND b.removed_at IS NULL")
		.bind(id)
		.bind(photo)
		.bind(owner)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a GBP's photo")
}

/// Google's country for GBP `id`, over one set by hand; another one drops the brand.
pub async fn set_country(conn: &mut SqliteConnection, id: i64, country: Country) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET country = $2, country_by = NULL, brand_id = CASE WHEN country IS $2 THEN brand_id END WHERE id = $1")
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
	sqlx::query("UPDATE gmbs SET country = $2, country_by = owner, brand_id = CASE WHEN country IS $2 THEN brand_id END WHERE id = $1")
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

/// Each of `ids` given `trade`, another one dropping the brand. `false`: one is no
/// GBP of the owner's not removed, and the others may be written: roll back.
pub async fn set_trades(conn: &mut SqliteConnection, owner: Uuid, ids: &BTreeSet<i64>, trade: Option<&str>) -> eyre::Result<bool> {
	let asked = i64::try_from(ids.len()).expect("a few ids");
	let ids = serde_json::to_string(ids).expect("ids serialize");
	let done = sqlx::query(
		"UPDATE gmbs SET trade = $3, brand_id = CASE WHEN trade IS $3 THEN brand_id END \
		 WHERE id IN (SELECT value FROM json_each($1)) AND owner = $2 AND removed_at IS NULL",
	)
	.bind(&ids)
	.bind(owner)
	.bind(trade)
	.execute(&mut *conn)
	.await
	.wrap_err("setting GMBs' trade")?;
	Ok(i64::try_from(done.rows_affected()).expect("a few rows") == asked)
}

/// What `before` → `after` logs: its moves, and the fields changed, as a Sync's or a hand's.
fn diff(before: &Gmb, after: &Gmb, synced: bool) -> Vec<Event> {
	assert_eq!(before.id, after.id, "one GBP's change");
	let mut events = Vec::new();
	if before.state != after.state {
		events.push(Event::State {
			from: before.state,
			to: after.state,
		});
	}
	if before.gmail_id != after.gmail_id {
		events.push(Event::Gmail {
			from: before.gmail_id,
			to: after.gmail_id,
		});
	}
	if before.trade != after.trade {
		events.push(Event::Trade {
			from: before.trade.clone().map(Traded::Gcid),
			to: after.trade.clone().map(Traded::Gcid),
		});
	}
	if before.brand_id != after.brand_id {
		events.push(Event::Brand {
			from: before.brand_id,
			to: after.brand_id,
		});
	}
	if before.film != after.film {
		events.push(Event::Film { film: after.film.clone() });
	}
	if before.enabled != after.enabled {
		events.push(Event::Switched { on: after.enabled });
	}
	let mut changes = Vec::new();
	let mut field = |field: Field, from: serde_json::Value, to: serde_json::Value| {
		if from != to {
			changes.push(Changed { field, from, to });
		}
	};
	fn json<T: serde::Serialize>(v: &T) -> serde_json::Value {
		serde_json::to_value(v).expect("a GBP's fields serialize")
	}
	field(Field::Name, json(&before.name), json(&after.name));
	field(Field::PlaceId, json(&before.place_id), json(&after.place_id));
	field(Field::Address, json(&before.address), json(&after.address));
	field(Field::Phone, json(&before.phone), json(&after.phone));
	field(Field::Website, json(&before.website), json(&after.website));
	field(Field::Hours, json(&before.hours), json(&after.hours));
	field(Field::GooglePrimary, json(&before.google_primary), json(&after.google_primary));
	field(Field::Country, json(&before.country), json(&after.country));
	field(Field::Point, json(&before.location), json(&after.location));
	let authors = |b: &Gmb| b.photos.iter().map(|p| p.author.clone()).collect::<Vec<_>>(); // a photo's id is new each Sync
	field(Field::Photos, json(&authors(before)), json(&authors(after)));
	field(Field::Areas, json(&before.areas), json(&after.areas));
	field(Field::HiddenAddress, json(&before.hidden_address), json(&after.hidden_address));
	field(Field::CustomersVisit, json(&before.customers_visit), json(&after.customers_visit));
	field(Field::Spots, json(&before.spots), json(&after.spots));
	field(Field::Role, json(&before.role), json(&after.role));
	field(Field::OwnerGmail, json(&before.owner_gmail), json(&after.owner_gmail));
	match synced {
		true => events.push(Event::Synced { changes }),
		false if !changes.is_empty() => events.push(Event::Edited { changes }),
		false => {}
	}
	events
}

#[derive(sqlx::FromRow)]
struct GmbRow {
	id: i64,
	owner: Uuid,
	gmail_id: Option<i64>,
	place_id: Option<String>,
	name: String,
	address: Option<String>,
	state: String,
	country: Option<String>,
	country_by_hand: bool,
	phone: Option<String>,
	trade: Option<String>,
	brand_id: Option<i64>,
	areas: Option<String>,
	hidden_address: Option<String>,
	customers_visit: Option<bool>,
	film: Option<String>,
	role: Option<String>,
	owner_gmail: Option<String>,
	lat: Option<f64>,
	lng: Option<f64>,
	placed: bool,
	created_at: i64,
	website: Option<String>,
	hours: Option<String>,
	google_primary: Option<String>,
	synced_at: Option<i64>,
	photos: String,
	enabled: bool,
	spots: String,
}

fn gmail((id, owner, gmail, created_at, browser, checked_at, error): GmailRow) -> eyre::Result<Gmail> {
	Ok(Gmail {
		id,
		owner,
		gmail,
		created_at: from_db(created_at)?,
		browser,
		imap: checked_at.map(|at| eyre::Ok(Checked { at: from_db(at)?, error })).transpose()?,
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
		owner: r.owner,
		gmail_id: r.gmail_id,
		place_id: r.place_id,
		name: r.name,
		address: r.address,
		state: r.state.parse()?,
		country: r.country.as_deref().map(str::parse).transpose()?.map(|code| GbpCountry { code, by_hand: r.country_by_hand }),
		phone: r.phone,
		trade: r.trade,
		brand_id: r.brand_id,
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
		role: r.role.as_deref().map(str::parse).transpose()?,
		owner_gmail: r.owner_gmail,
		created_at: from_db(r.created_at)?,
		enabled: r.enabled,
		location,
		spots: serde_json::from_str(&r.spots).wrap_err_with(|| format!("GMB {}'s spots", r.id))?,
		website: r.website,
		hours: r
			.hours
			.as_deref()
			.map(serde_json::from_str)
			.transpose()
			.wrap_err_with(|| format!("GMB {}'s hours", r.id))?
			.unwrap_or_default(),
		google_primary: r.google_primary,
		photos: serde_json::from_str(&r.photos).wrap_err_with(|| format!("GMB {}'s photos", r.id))?,
		synced_at: r.synced_at.map(from_db).transpose()?,
	})
}
