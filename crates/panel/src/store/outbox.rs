//! The alerts' outbox: what each destination is owed, at its network's pace. Every timestamp is the
//! caller's `now`, never the database's clock, so the pacing can be driven by a test's virtual time.

use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use sa_alerts::{Rule, Tier};
use sa_core::{
	alerts::{Due as When, Method},
	notify::{GLOBAL_PER_SECOND, OutboxId, PER_CHAT_GAP, PER_GROUP_GAP},
};
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{begin_write, to_db};

// A macro, not a const, so every query stays a literal (`concat!`) that sqlx takes as audited.
/// Gives up what the outbox still owed: the filter follows.
macro_rules! give_up {
	() => {
		"UPDATE alert_outbox SET state = 'dropped', leased_until = NULL, last_error = $1 WHERE state = 'pending' AND "
	};
}

/// A message to queue.
pub struct New<'a> {
	pub destination_id: i64,
	pub user_id: Uuid,
	pub note_id: i64,
	pub rule: Rule,
	pub tier: Tier,
	pub lane: &'a str,
	pub when: When,
	/// When what it tells of was noted: the stale rule counts from it.
	pub noted_at: Timestamp,
}

pub async fn enqueue(conn: &mut SqliteConnection, m: &New<'_>) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO alert_outbox (destination_id, user_id, note_id, rule, tier, together, lane, next_attempt_at, created_at) \
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
	)
	.bind(m.destination_id)
	.bind(m.user_id)
	.bind(m.note_id)
	.bind(m.rule.as_ref())
	.bind(m.tier.as_ref())
	.bind(m.when.together)
	.bind(m.lane)
	.bind(to_db(m.when.at))
	.bind(to_db(m.noted_at))
	.execute(&mut *conn)
	.await
	.wrap_err("queueing a message")?;
	Ok(())
}

/// A message claimed for sending.
#[derive(Clone, Debug)]
pub struct Due {
	pub id: i64,
	pub destination_id: i64,
	pub user_id: Uuid,
	pub note_id: i64,
	pub rule: Rule,
	pub tier: Tier,
	pub together: bool,
	/// Tries so far, this one included.
	pub attempts: i32,
}

/// Claims what may be sent at `now`, leased until `lease_until`: at most one message per lane that
/// has had none in the last [`PER_CHAT_GAP`] (a private chat) or [`PER_GROUP_GAP`] (any other) and
/// none in flight, and of a method not paused no more than [`GLOBAL_PER_SECOND`] tries started in
/// the last second — whoever else claims, since every claim reads the pace and leases inside one
/// write transaction, one at a time. Most urgent first, then oldest; one that goes out together
/// brings along the rest due at its destination.
pub async fn claim_due(conn: &mut SqliteConnection, now: Timestamp, lease_until: Timestamp) -> eyre::Result<Vec<Due>> {
	let mut tx = begin_write(&mut *conn).await?;
	type Row = (i64, i64, Uuid, i64, String, String, bool, i32);
	let rows: Vec<Row> = sqlx::query_as(
		"WITH busy AS ( \
		   SELECT o.lane FROM alert_outbox o LEFT JOIN telegram_destinations t ON t.destination_id = o.destination_id \
		   WHERE o.attempted_at > $5 AND o.attempted_at <= $1 AND o.attempted_at > iif(t.kind = 'private', $2, $5) \
		   UNION SELECT lane FROM alert_outbox WHERE state = 'pending' AND leased_until > $1 \
		 ), due AS ( \
		   SELECT o.*, d.method, CASE o.tier WHEN 'urgent' THEN 0 WHEN 'important' THEN 1 ELSE 2 END AS rank \
		   FROM alert_outbox o JOIN alert_destinations d ON d.id = o.destination_id \
		   WHERE o.state = 'pending' AND o.next_attempt_at <= $1 AND (o.leased_until IS NULL OR o.leased_until <= $1) \
		     AND o.lane NOT IN (SELECT lane FROM busy) \
		     AND NOT EXISTS (SELECT 1 FROM alert_pauses p WHERE p.method = d.method AND p.until > $1) \
		 ), pick AS ( \
		   SELECT *, row_number() OVER (PARTITION BY lane ORDER BY rank, id) AS place FROM due \
		 ), paced AS ( \
		   SELECT *, row_number() OVER (PARTITION BY method ORDER BY rank, id) AS turn FROM pick WHERE place = 1 \
		 ), chosen AS ( \
		   SELECT * FROM paced p WHERE turn <= $4 - ( \
		     SELECT count(*) FROM alert_outbox s JOIN alert_destinations d ON d.id = s.destination_id \
		     WHERE d.method = p.method AND s.attempted_at > $6 AND s.attempted_at <= $1) \
		 ) \
		 UPDATE alert_outbox SET leased_until = $3, attempted_at = $1, attempts = attempts + 1 \
		 WHERE id IN (SELECT id FROM chosen) \
		    OR id IN (SELECT a.id FROM due a JOIN chosen c ON c.destination_id = a.destination_id AND c.together AND a.together) \
		 RETURNING id, destination_id, user_id, note_id, rule, tier, together, attempts",
	)
	.bind(to_db(now))
	.bind(to_db(now - PER_CHAT_GAP))
	.bind(to_db(lease_until))
	.bind(GLOBAL_PER_SECOND)
	.bind(to_db(now - PER_GROUP_GAP))
	.bind(to_db(now - SignedDuration::from_secs(1)))
	.fetch_all(&mut *tx)
	.await
	.wrap_err("claiming due messages")?;
	tx.commit().await.wrap_err("committing a claim")?;
	let mut due = rows
		.into_iter()
		.map(|(id, destination_id, user_id, note_id, rule, tier, together, attempts)| {
			Ok(Due {
				id,
				destination_id,
				user_id,
				note_id,
				rule: rule.parse().wrap_err_with(|| format!("message {id}'s rule"))?,
				tier: tier.parse().wrap_err_with(|| format!("message {id}'s tier"))?,
				together,
				attempts,
			})
		})
		.collect::<eyre::Result<Vec<_>>>()?;
	due.sort_by_key(|d| (std::cmp::Reverse(d.tier), d.id));
	Ok(due)
}

/// `together`: in one message with others, so no button of it is pressed.
pub async fn sent(conn: &mut SqliteConnection, id: i64, remote: &str, together: bool, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("UPDATE alert_outbox SET state = 'sent', sent_at = $3, remote_id = $2, together = $4, leased_until = NULL, last_error = NULL WHERE id = $1")
		.bind(id)
		.bind(remote)
		.bind(to_db(now))
		.bind(together)
		.execute(&mut *conn)
		.await
		.wrap_err("recording a sent message")?;
	Ok(())
}

/// A failed try, tried again at `at`.
pub async fn retry(conn: &mut SqliteConnection, id: i64, at: Timestamp, error: &str) -> eyre::Result<()> {
	sqlx::query("UPDATE alert_outbox SET next_attempt_at = $2, leased_until = NULL, last_error = $3 WHERE id = $1")
		.bind(id)
		.bind(to_db(at))
		.bind(error)
		.execute(&mut *conn)
		.await
		.wrap_err("scheduling a retry")?;
	Ok(())
}

/// A network asked to wait (429): nothing of `method` leaves the outbox before `until`, and the try
/// is not counted against the message — the wait was ours, not its address's.
pub async fn pause(conn: &mut SqliteConnection, id: i64, method: Method, until: Timestamp, error: &str) -> eyre::Result<()> {
	let until = to_db(until);
	sqlx::query("INSERT INTO alert_pauses (method, until) VALUES ($1, $2) ON CONFLICT (method) DO UPDATE SET until = max(until, $2)")
		.bind(method.as_ref())
		.bind(until)
		.execute(&mut *conn)
		.await
		.wrap_err("pausing a method")?;
	sqlx::query("UPDATE alert_outbox SET attempts = attempts - 1, next_attempt_at = $2, leased_until = NULL, last_error = $3 WHERE id = $1")
		.bind(id)
		.bind(until)
		.bind(error)
		.execute(&mut *conn)
		.await
		.wrap_err("putting a message back")?;
	Ok(())
}

/// Given up on: the tries ran out, or no try can succeed.
pub async fn dead(conn: &mut SqliteConnection, id: i64, error: &str) -> eyre::Result<()> {
	sqlx::query("UPDATE alert_outbox SET state = 'dead', leased_until = NULL, last_error = $2 WHERE id = $1")
		.bind(id)
		.bind(error)
		.execute(&mut *conn)
		.await
		.wrap_err("giving up on a message")?;
	Ok(())
}

/// Gives up on lead messages noted before `before`: past it they are news nobody needs.
pub async fn drop_stale(conn: &mut SqliteConnection, before: Timestamp) -> eyre::Result<()> {
	sqlx::query(concat!(give_up!(), "rule IN ('new_lead', 'contact_overdue') AND NOT together AND created_at < $2"))
		.bind("stale")
		.bind(to_db(before))
		.execute(&mut *conn)
		.await
		.wrap_err("expiring stale messages")?;
	Ok(())
}

/// Past `over` of one rule's messages waiting for one destination, they go out together.
pub async fn fold_crowded(conn: &mut SqliteConnection, over: i64, now: Timestamp) -> eyre::Result<()> {
	sqlx::query(
		"UPDATE alert_outbox SET together = 1 WHERE state = 'pending' AND NOT together AND next_attempt_at <= $2 AND (leased_until IS NULL OR leased_until <= $2) \
		 AND (destination_id, rule) IN ( \
		   SELECT destination_id, rule FROM alert_outbox \
		   WHERE state = 'pending' AND NOT together AND next_attempt_at <= $2 AND (leased_until IS NULL OR leased_until <= $2) \
		   GROUP BY destination_id, rule HAVING count(*) > $1)",
	)
	.bind(over)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err("folding a crowd of messages")?;
	Ok(())
}

/// A message not tried yet, held until later.
pub struct Waiting {
	pub id: i64,
	pub destination_id: i64,
	pub rule: Rule,
	pub tier: Tier,
}

/// What waits of the user's past `now`.
pub async fn waiting(conn: &mut SqliteConnection, user: Uuid, now: Timestamp) -> eyre::Result<Vec<Waiting>> {
	let rows: Vec<(i64, i64, String, String)> =
		sqlx::query_as("SELECT id, destination_id, rule, tier FROM alert_outbox WHERE user_id = $1 AND state = 'pending' AND attempts = 0 AND next_attempt_at > $2")
			.bind(user)
			.bind(to_db(now))
			.fetch_all(&mut *conn)
			.await
			.wrap_err("listing what waits")?;
	rows.into_iter()
		.map(|(id, destination_id, rule, tier)| {
			Ok(Waiting {
				id,
				destination_id,
				rule: rule.parse().wrap_err_with(|| format!("message {id}'s rule"))?,
				tier: tier.parse().wrap_err_with(|| format!("message {id}'s tier"))?,
			})
		})
		.collect()
}

pub async fn postpone(conn: &mut SqliteConnection, id: i64, when: When) -> eyre::Result<()> {
	sqlx::query("UPDATE alert_outbox SET next_attempt_at = $2, together = $3 WHERE id = $1")
		.bind(id)
		.bind(to_db(when.at))
		.bind(when.together)
		.execute(&mut *conn)
		.await
		.wrap_err("moving what waits")?;
	Ok(())
}

pub async fn drop_one(conn: &mut SqliteConnection, id: i64, why: &str) -> eyre::Result<()> {
	sqlx::query(concat!(give_up!(), "id = $2"))
		.bind(why)
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("dropping what waits")?;
	Ok(())
}

/// Gives up what was still waiting to go out of the notes the user has read.
pub(super) async fn drop_read(conn: &mut SqliteConnection, user: Uuid, now: Timestamp) -> eyre::Result<()> {
	sqlx::query(concat!(
		give_up!(),
		"user_id = $2 AND next_attempt_at > $3 AND attempts = 0 \
		 AND EXISTS (SELECT 1 FROM alert_inbox i WHERE i.user_id = alert_outbox.user_id AND i.note_id = alert_outbox.note_id AND i.read_at IS NOT NULL)"
	))
	.bind("read")
	.bind(user)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err("dropping what was read")?;
	Ok(())
}

/// Gives up what the outbox still owed a user, and why.
pub(super) async fn drop_of_user(conn: &mut SqliteConnection, user: Uuid, why: &str) -> eyre::Result<()> {
	sqlx::query(concat!(give_up!(), "user_id = $2"))
		.bind(why)
		.bind(user)
		.execute(&mut *conn)
		.await
		.wrap_err_with(|| format!("dropping what a user was owed ({why})"))?;
	Ok(())
}

/// Gives up what the outbox still owed destinations nothing reaches.
pub(super) async fn drop_of_dead(conn: &mut SqliteConnection, why: &str) -> eyre::Result<()> {
	sqlx::query(concat!(give_up!(), "destination_id IN (SELECT id FROM alert_destinations WHERE dead_at IS NOT NULL)"))
		.bind(why)
		.execute(&mut *conn)
		.await
		.wrap_err("dropping what a dead destination was owed")?;
	Ok(())
}

/// Gives up what the outbox still owed a destination, and why.
pub async fn drop_of_destination(conn: &mut SqliteConnection, destination: i64, why: &str) -> eyre::Result<()> {
	sqlx::query(concat!(give_up!(), "destination_id = $2"))
		.bind(why)
		.bind(destination)
		.execute(&mut *conn)
		.await
		.wrap_err("dropping what a destination was owed")?;
	Ok(())
}

/// A sent message: whose, where, and of which note.
#[derive(Clone, Debug)]
pub struct Sent {
	pub user_id: Uuid,
	pub note_id: i64,
	pub lane: String,
}

/// The message `id`, if it was sent on its own.
pub async fn message(conn: &mut SqliteConnection, OutboxId { v: id }: OutboxId) -> eyre::Result<Option<Sent>> {
	let row: Option<(Uuid, i64, String)> = sqlx::query_as("SELECT user_id, note_id, lane FROM alert_outbox WHERE id = $1 AND state = 'sent' AND NOT together")
		.bind(id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("reading a sent message")?;
	Ok(row.map(|(user_id, note_id, lane)| Sent { user_id, note_id, lane }))
}

/// What became of a note's message to one of a person's destinations.
#[derive(Clone, Debug, Eq, PartialEq, schemars::JsonSchema, serde::Serialize)]
pub struct Told {
	pub destination_id: i64,
	pub state: State,
	/// When it was sent, or will be.
	pub at: Timestamp,
	pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::EnumString, schemars::JsonSchema, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[schemars(rename = "ToldState")]
pub enum State {
	Pending,
	Sent,
	/// No try reached it.
	Dead,
	/// Given up before a try: read first, or stale.
	Dropped,
}

/// Where a note of the user's went, or will go.
pub async fn told(conn: &mut SqliteConnection, user: Uuid, note: i64) -> eyre::Result<Vec<Told>> {
	type Row = (i64, String, i64, Option<String>);
	let rows: Vec<Row> = sqlx::query_as(
		"SELECT destination_id, state, coalesce(sent_at, next_attempt_at), last_error FROM alert_outbox WHERE user_id = $1 AND note_id = $2 ORDER BY destination_id",
	)
	.bind(user)
	.bind(note)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("reading where a note went")?;
	rows.into_iter()
		.map(|(destination_id, state, at, error)| {
			Ok(Told {
				destination_id,
				state: state.parse().wrap_err("a stored state")?,
				at: super::from_db(at)?,
				error,
			})
		})
		.collect()
}
