import { api, ok } from "@/shared/api";
import type { Category, Country, GoogleCategory, Release } from "@/shared/api/schema";

import { emptySettings } from "../model/category";

const at = (trade: string, country: Country) => ({ params: { path: { trade, country } } });

/** The caller's category of `trade` in `country`, set up off Google's list with nothing but the trade's name. */
export function setUpCategory(trade: GoogleCategory, country: Country): Promise<Category> {
  return ok(api.PUT("/categories/{trade}/{country}", { ...at(trade.id, country), body: emptySettings(trade, country) }));
}

/** The head moved onto a listing's release: an adopt, or an update along its lineage; the category set up if it was not. */
export function adoptRelease(trade: string, country: Country, release: Release): Promise<Category> {
  return ok(api.POST("/categories/{trade}/{country}/head", { ...at(trade, country), body: { version: release.version, listing_version: release.id } }));
}

/** The head moved back onto a version of its own history. */
export function revertCategory(trade: string, country: Country, version: number): Promise<Category> {
  return ok(api.POST("/categories/{trade}/{country}/head", { ...at(trade, country), body: { version } }));
}
