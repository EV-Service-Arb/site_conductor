import { http, ignoreBody } from "@/shared/api";
import { type Parser, nullable, num, object, str } from "@/shared/lib/parse";

import type { Checked, Connections, Imap, MapsKey, Prices } from "../model/generated";

const checkedParser: Parser<Checked> = object({ at: str, error: nullable(str) });
const imapParser: Parser<Imap> = object({ host: str, username: str, set_at: str, checked: checkedParser });
const mapsKeyParser: Parser<MapsKey> = object({ last4: str, set_at: str, checked: checkedParser, out: nullable(str) });
export const pricesParser: Parser<Prices> = object({ details: nullable(num), search: nullable(num) });
const connectionsParser: Parser<Connections> = object({ imap: nullable(imapParser), maps_key: nullable(mapsKeyParser), prices: pricesParser });

export type Connection = "imap" | "maps_key";

export function fetchConnections(gmail: number): Promise<Connections> {
  return http.get(`/api/v1/gmails/${gmail}/connections`, connectionsParser);
}

/** Kept only once the inbox logs in; refused with what its server said. */
export function setImap(gmail: number, imap: { host: string; username: string; password: string }): Promise<Connections> {
  return http.send("PUT", `/api/v1/gmails/${gmail}/imap`, imap, connectionsParser);
}

/** Kept only once Google answers for it; refused with Google's reason. */
export function setMapsKey(gmail: number, key: string): Promise<Connections> {
  return http.send("PUT", `/api/v1/gmails/${gmail}/maps_key`, { key }, connectionsParser);
}

export function testConnection(gmail: number, which: Connection): Promise<Connections> {
  return http.send("POST", `/api/v1/gmails/${gmail}/${which}/test`, undefined, connectionsParser);
}

export async function disconnect(gmail: number, which: Connection): Promise<void> {
  await http.send("DELETE", `/api/v1/gmails/${gmail}/${which}`, undefined, ignoreBody);
}

export function fetchPrices(): Promise<Prices> {
  return http.get("/api/v1/admin/tokens/prices", pricesParser);
}

export function setPrices(prices: { details: number; search: number }): Promise<Prices> {
  return http.send("PUT", "/api/v1/admin/tokens/prices", prices, pricesParser);
}
