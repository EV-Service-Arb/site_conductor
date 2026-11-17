//! The areas' rows (`migrations/…_areas_circles.up.sql`).

use eyre::WrapErr;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};
use crate::areas::Area;

const AREA: &str = "SELECT slug, name, country, city, lat, lng, radius_m, created_by, created_at FROM areas";

type AreaRow = (String, String, String, Option<String>, f64, f64, i64, Uuid, i64);

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
	sqlx::query(
		"INSERT INTO areas (slug, name, country, city, lat, lng, radius_m, created_by, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
		 ON CONFLICT (slug) DO UPDATE SET name = excluded.name, country = excluded.country, city = excluded.city, lat = excluded.lat, \
		 lng = excluded.lng, radius_m = excluded.radius_m",
	)
	.bind(&a.slug)
	.bind(&a.name)
	.bind(&a.country)
	.bind(&a.city)
	.bind(a.center.lat)
	.bind(a.center.lng)
	.bind(a.radius_m)
	.bind(a.created_by)
	.bind(to_db(a.created_at))
	.execute(&mut *conn)
	.await
	.wrap_err("writing an area")?;
	Ok(())
}

fn area_of((slug, name, country, city, lat, lng, radius_m, created_by, created_at): AreaRow) -> eyre::Result<Area> {
	Ok(Area {
		slug,
		name,
		country,
		center: sa_places::LatLng { lat, lng },
		radius_m,
		city,
		created_by,
		created_at: from_db(created_at)?,
	})
}
