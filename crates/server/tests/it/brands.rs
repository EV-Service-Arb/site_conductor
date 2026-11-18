//! A brand sits under one gmail and one category: a GBP holds one only of its own gmail and
//! category, drops it when either moves, and a brand a live GBP holds stays. Its photos are what
//! its GBPs' drafts are offered with.

use axum::http::StatusCode;
use jiff::{SignedDuration, Timestamp};
use sa_core::{gbp::Country, ids::BrandId};
use sa_panel::{Refused, brands::NewBrand, gmbs::Edit, testing::TestDb};
use serde_json::json;

use super::kit::{kit, png};

#[tokio::test]
async fn a_gbp_holds_a_brand_of_its_gmail_and_category_only_and_drops_it_when_either_moves() {
	let db = TestDb::create().await;
	let k = kit(&db).await;
	let now = Timestamp::now();
	let paris = k.panel.add_gmail(k.owner, "paris@gmail.com", now).await.unwrap().unwrap().id;
	let theirs = k
		.panel
		.add_brand(
			k.owner,
			NewBrand {
				gmail_id: paris,
				trade_id: k.trade,
				country: Country::FR,
				name: "Aquafix".to_owned(),
				site: BrandId::parse("aquafix").unwrap(),
			},
			now,
		)
		.await
		.unwrap()
		.expect("one name per gmail × category: another gmail's may share it");

	let refused = k.panel.set_brands(k.owner, &[k.seven], Some(theirs.id)).await.unwrap();
	assert!(matches!(refused, Err(Refused::Conflict(_))), "another gmail's brand: {refused:?}");
	let brand_of = async |gmb: i64| k.panel.gmails(k.owner).await.unwrap().1.into_iter().find(|g| g.id == gmb).unwrap().brand_id;
	assert_eq!(brand_of(k.seven).await, Some(k.brand), "nothing written");

	let moved = Edit {
		gmail_id: Some(paris),
		..Edit::default()
	};
	k.panel.edit_gmb(k.owner, k.seven, moved).await.unwrap().unwrap();
	assert_eq!(brand_of(k.seven).await, None, "a transfer drops the brand");
	k.panel.set_brands(k.owner, &[k.seven], Some(theirs.id)).await.unwrap().unwrap();
	assert_eq!(brand_of(k.seven).await, Some(theirs.id));

	k.panel.set_trades(k.owner, &[k.three], None).await.unwrap().unwrap();
	assert_eq!(brand_of(k.three).await, None, "a trade taken away drops the brand");
	let refused = k.panel.set_brands(k.owner, &[k.three], Some(k.brand)).await.unwrap();
	assert!(matches!(refused, Err(Refused::Conflict(_))), "no category, no brand: {refused:?}");
}

#[tokio::test]
async fn a_brand_a_live_gbp_holds_stays() {
	let db = TestDb::create().await;
	let k = kit(&db).await;
	let now = Timestamp::now();

	let refused = k.panel.remove_brand(k.owner, k.brand, now).await.unwrap();
	assert!(matches!(refused, Err(Refused::Conflict(_))), "{refused:?}");
	k.panel.remove_gmb(k.owner, k.seven, now).await.unwrap().unwrap();
	k.panel.set_brands(k.owner, &[k.three], None).await.unwrap().unwrap();
	k.panel.remove_brand(k.owner, k.brand, now).await.unwrap().unwrap();
	assert!(k.panel.brands(k.owner).await.unwrap().is_empty());
	let refused = k.panel.remove_category(k.owner, k.trade, Country::FR).await.unwrap();
	assert!(matches!(refused, Err(Refused::Conflict(_))), "a brand, removed too, keeps its category: {refused:?}");
}

#[tokio::test]
async fn a_branded_gbps_drafts_carry_its_brands_photos_the_less_used_first_never_one_saved_there() {
	let db = TestDb::create().await;
	let k = kit(&db).await;
	let third = k.panel.add_photo(k.owner, k.brand, png(30), Timestamp::now()).await.unwrap().unwrap();

	let (view, offered) = k.offer("lyon-7").await;
	let photos: Vec<Vec<i64>> = offered.iter().map(|(_, p)| p.clone()).collect();
	assert_eq!(photos[0], [k.photos[0], k.photos[1]], "two each, the least used first");
	assert_eq!(photos[1], [third, k.photos[0]], "and on round the brand's");
	assert_eq!(k.event(json!({ "view": view, "saved": [k.photos[0], third] })).await, StatusCode::NO_CONTENT);

	let later = Timestamp::now() + SignedDuration::from_mins(16);
	let again = k.panel.kit_offer(k.seven, true, None, later).await.unwrap().unwrap();
	assert!(again.drafts.iter().all(|d| d.photos.iter().map(|p| p.id).eq([k.photos[1]])), "only the one not saved there");
	let elsewhere = k.panel.kit_offer(k.three, true, None, later).await.unwrap().unwrap();
	assert_eq!(
		elsewhere.drafts[0].photos.iter().map(|p| p.id).collect::<Vec<_>>(),
		[k.photos[1], k.photos[0]],
		"saved elsewhere ranks lower"
	);

	k.panel.set_brands(k.owner, &[k.three], None).await.unwrap().unwrap();
	let (other, offered) = k.offer("lyon-3").await;
	assert!(offered.iter().all(|(_, p)| p.is_empty()), "no brand, no photos");
	let (status, _) = k.get(&format!("/api/kit/v1/photos/{}.jpg?view={other}", k.photos[1])).await;
	assert_eq!(status, StatusCode::NOT_FOUND, "not its brand's");
}
