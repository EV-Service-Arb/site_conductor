import type { Category, Country, Gmb, Trade } from "./gmb";

/** A GBP's category: its trade in its own country, set up there or not; never another country's. */
export type Placed =
  | { status: "no_trade" }
  | { status: "no_country"; trade: Trade }
  | { status: "not_set_up"; trade: Trade; country: Country }
  | { status: "set_up"; trade: Trade; category: Category };

export function categoryOf(gmb: Gmb, trades: readonly Trade[]): Placed {
  if (gmb.trade_id === null) return { status: "no_trade" };
  const trade = trades.find((t) => t.id === gmb.trade_id);
  if (trade === undefined) throw new Error(`GBP ${gmb.id}'s trade ${gmb.trade_id} is not among the trades read`);
  if (gmb.country === null) return { status: "no_country", trade };
  const category = trade.categories.find((c) => c.country === gmb.country!.code);
  return category === undefined ? { status: "not_set_up", trade, country: gmb.country.code } : { status: "set_up", trade, category };
}

/** "Plumbing · FR" */
export function categoryName(trade: Trade, country: Country): string {
  return `${trade.name} · ${country}`;
}
