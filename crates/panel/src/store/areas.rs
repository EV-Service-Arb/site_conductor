//! The areas' rows (`migrations/…_areas.up.sql`).

use eyre::WrapErr;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};
use crate::areas::{Area, Shape};

const AREA: &str = "SELECT slug, kind, name, country, place_id, lat, lng, radius_m, south, west, north, east, margin_m, created_by, created_at FROM areas";

type AreaRow = (
	String,
	String,
	String,
	String,
	Option<String>,
	f64,
	f64,
	Option<i64>,
	Option<f64>,
	Option<f64>,
	Option<f64>,
	Option<f64>,
	i64,
	Uuid,
	i64,
);

pub async fn area(conn: &mut SqliteConnection, slug: &str) -> eyre::Result<Option<Area>> {
	let row: Option<AreaRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{AREA} WHERE slug = $1")))
		.bind(slug)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading an area")?;
	row.map(area_of).transpose()
}

pub async fn areas(conn: &mut SqliteConnection) -> eyre::Result<Vec<Area>> {
	let rows: Vec<AreaRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{AREA} ORDER BY slug")))
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing areas")?;
	rows.into_iter().map(area_of).collect()
}

/// Over what the slug held.
pub async fn put_area(conn: &mut SqliteConnection, a: &Area) -> eyre::Result<()> {
	let (kind, place_id, radius, bounds) = match &a.shape {
		Shape::City { place_id, viewport } => ("city", Some(place_id.as_str()), None, Some(viewport)),
		Shape::Circle { radius_m } => ("circle", None, Some(*radius_m), None),
	};
	sqlx::query(
		"INSERT INTO areas (slug, kind, name, country, place_id, lat, lng, radius_m, south, west, north, east, margin_m, created_by, created_at) \
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
		 ON CONFLICT (slug) DO UPDATE SET kind = excluded.kind, name = excluded.name, country = excluded.country, place_id = excluded.place_id, lat = excluded.lat, \
		 lng = excluded.lng, radius_m = excluded.radius_m, south = excluded.south, west = excluded.west, north = excluded.north, \
		 east = excluded.east, margin_m = excluded.margin_m",
	)
	.bind(&a.slug)
	.bind(kind)
	.bind(&a.name)
	.bind(&a.country)
	.bind(place_id)
	.bind(a.center.lat)
	.bind(a.center.lng)
	.bind(radius)
	.bind(bounds.map(|b| b.south))
	.bind(bounds.map(|b| b.west))
	.bind(bounds.map(|b| b.north))
	.bind(bounds.map(|b| b.east))
	.bind(a.margin_m)
	.bind(a.created_by)
	.bind(to_db(a.created_at))
	.execute(&mut *conn)
	.await
	.wrap_err("writing an area")?;
	Ok(())
}

fn area_of((slug, kind, name, country, place_id, lat, lng, radius_m, south, west, north, east, margin_m, created_by, created_at): AreaRow) -> eyre::Result<Area> {
	let shape = match (kind.as_str(), place_id, radius_m, south, west, north, east) {
		("city", Some(place_id), None, Some(south), Some(west), Some(north), Some(east)) => Shape::City {
			place_id,
			viewport: sa_places::Viewport { south, west, north, east },
		},
		("circle", None, Some(radius_m), None, None, None, None) => Shape::Circle { radius_m },
		_ => eyre::bail!("area {slug} is neither a city nor a circle"),
	};
	Ok(Area {
		slug,
		name,
		country,
		center: sa_places::LatLng { lat, lng },
		shape,
		margin_m,
		created_by,
		created_at: from_db(created_at)?,
	})
}
