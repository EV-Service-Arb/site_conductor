//! Street View on the browser's Maps key: each panorama opened is bought here first, at the
//! admin's price of [`StreetViewLoad::Panorama`], and none past Google's free ones in its billing month.
//!
//! ```text
//! the pegman dropped ─ a panorama there? (free) ─ open(user): month < FREE, price set, balance ≥ price,
//!   recorded and charged in one write ─ the panorama opened in the browser
//! ```

use eyre::WrapErr;
use jiff::{
	Timestamp,
	tz::{TimeZone, offset},
};
use sa_core::{
	prices::{Item, StreetViewLoad},
	usage::Bill,
};
use uuid::Uuid;

use crate::{
	Panel, Refused,
	store::{
		tokens::{self as ledger, Charged, Entry, Kind},
		usage::{self, CONDUCTOR, Use},
	},
	tokens::is_open,
};

/// Dynamic Street View's free loads a month, per billing account.
pub const FREE: i64 = 5_000;

/// Google's billing month starts at Pacific midnight; PDT's, the earlier, is counted from.
fn month_start(now: Timestamp) -> eyre::Result<Timestamp> {
	let pacific = TimeZone::fixed(offset(-7));
	Ok(now.to_zoned(pacific.clone()).date().first_of_month().to_zoned(pacific).wrap_err("a month's start")?.timestamp())
}

impl Panel {
	/// The user opens a panorama at `lat, lng`: bought, or refused before anything loads. Their balance then.
	pub async fn open_street_view(&self, user: Uuid, lat: f64, lng: f64, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
			return Ok(Err(Refused::Invalid(format!("{lat}, {lng} is no point on Earth"))));
		}
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let item = Item::from(StreetViewLoad::Panorama);
		if usage::units_since(&mut tx, item, month_start(now)?).await? >= FREE {
			return Ok(Err(Refused::Conflict(format!(
				"Street View's free {FREE} this month are used: the map shows where it is, until the month is out"
			))));
		}
		let Some(price) = ledger::price(&mut tx, item).await? else {
			return Ok(Err(Refused::Conflict("Street View is not priced yet: ask an admin to price it".to_owned())));
		};
		let balance = ledger::renewed(&mut tx, user, now, &self.renewal).await?;
		if balance < price {
			return Ok(Err(Refused::Conflict(format!("Street View costs {price} tokens, and you hold {balance}"))));
		}
		let key = format!("{lat:.6},{lng:.6}");
		let opened = Use {
			at: now,
			item,
			units: 1,
			bill: Bill::Ours,
			source: CONDUCTOR.to_owned(),
			user: Some(user),
			key: Some(key.clone()),
		};
		usage::record(&mut tx, &opened).await?;
		if price > 0 {
			let entry = Entry {
				at: now,
				delta: -price,
				kind: Kind::Charge,
				charged: Some(Charged {
					service: item.tool().to_owned(),
					key,
					label: Some(format!("Street View · {lat:.5}, {lng:.5}")),
				}),
				by: None,
				note: None,
			};
			ledger::append(&mut tx, user, &entry).await?;
		}
		tx.commit().await.wrap_err("committing a panorama bought")?;
		tracing::info!(user_id = %user, tokens = price, "Street View opened");
		if price > 0 {
			self.snapshot_due.notify_one();
		}
		Ok(Ok(balance - price))
	}
}
