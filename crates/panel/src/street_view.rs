//! Street View on an account's browser key: each panorama opened is bought here first, at the
//! admin's price of [`StreetViewLoad::Panorama`], and none past its meter's free ones there.
//!
//! ```text
//! the pegman dropped ─ a panorama there? (free) ─ open(user, account): its meter there not stopped, price set,
//!   balance ≥ price, recorded and charged in one write ─ the panorama opened in the browser
//! ```

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	prices::{Item, Spender, StreetViewLoad},
	usage::Bill,
};

use crate::{
	Panel, Refused, meters,
	store::{
		tokens::{self as ledger, Charged, Entry, Kind},
		usage::{self, CONDUCTOR, Use},
	},
	tokens::is_open,
};

impl Panel {
	/// The user opens a panorama at `lat, lng` on `account`'s browser key: bought, or refused before
	/// anything loads. Their balance then.
	pub async fn open_street_view(&self, by: Spender<StreetViewLoad>, account: &str, lat: f64, lng: f64, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		let user = by.user();
		if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
			return Ok(Err(Refused::Invalid(format!("{lat}, {lng} is no point on Earth"))));
		}
		if self.accounts.get(account).is_none_or(|a| a.provides.browser.is_none()) {
			return Ok(Err(Refused::Invalid(format!("{account:?} is no Google account with a browser key"))));
		}
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let item = Item::from(StreetViewLoad::Panorama);
		let meter = item.meter().expect("a panorama is on Google's meter");
		if let Some(why) = meters::gauge(&mut tx, account, meter, now).await?.refuses(1) {
			return Ok(Err(Refused::Conflict(why)));
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
			account: Some(account.to_owned()),
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
