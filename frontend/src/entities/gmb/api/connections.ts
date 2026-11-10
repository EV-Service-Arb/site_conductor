import { ApiError, http, ignoreBody } from "@/shared/api";
import { type Parser, bool, nullable, num, object, oneOf, str } from "@/shared/lib/parse";

import { type Browser, type Checked, type Connections, type Imap, type MapsKey, type Prices, PROVIDERS, type SetBrowser } from "../model/generated";

const checkedParser: Parser<Checked> = object({ at: str, error: nullable(str) });
const imapParser: Parser<Imap> = object({ host: str, username: str, set_at: str, checked: checkedParser });
const mapsKeyParser: Parser<MapsKey> = object({ last4: str, set_at: str, checked: checkedParser, out: nullable(str) });
export const pricesParser: Parser<Prices> = object({ details: nullable(num), search: nullable(num) });
const browserParser: Parser<Browser> = object({
  provider: oneOf(PROVIDERS),
  device: str,
  endpoint: str,
  exit: object({ ip: str, country: str, city: str, zone: str, mobile: bool }),
  set_at: str,
  checked: checkedParser,
  running: nullable(object({ gmb: nullable(num), stream: nullable(str) })),
  film: bool,
});
const connectionsParser: Parser<Connections> = object({ imap: nullable(imapParser), maps_key: nullable(mapsKeyParser), prices: pricesParser, browser: nullable(browserParser) });

export type Connection = "imap" | "maps_key" | "browser";

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

/** Every browser route's 503 while the server has no browser provider configured. */
export function browsersUnconnected(e: unknown): boolean {
  return e instanceof ApiError && e.failure.kind === "unavailable" && e.failure.message === "browsers not connected";
}

/** Kept only once the proxy exits in the country of the gmail's GBPs; the provider and device are set once, the proxy replaced under them. */
export function setBrowser(gmail: number, browser: SetBrowser): Promise<Connections> {
  return http.send("PUT", `/api/v1/gmails/${gmail}/browser`, browser, connectionsParser);
}

/** Answers once the browser is up, at `gmb`'s point; `null`: no GBP, the position blocked. */
export async function startBrowser(gmail: number, gmb: number | null): Promise<void> {
  await http.send("POST", `/api/v1/gmails/${gmail}/browser/start`, { gmb }, ignoreBody);
}

export async function stopBrowser(gmail: number): Promise<void> {
  await http.send("POST", `/api/v1/gmails/${gmail}/browser/stop`, undefined, ignoreBody);
}

/** Played as the back camera by the browser's next launch; refused while it runs, or when ffmpeg cannot cut it. */
export function uploadFilm(gmail: number, film: Blob, onProgress: (sent: number) => void): Promise<void> {
  return http.upload(`/api/v1/gmails/${gmail}/browser/film`, film, "video/mp4", onProgress);
}

export async function removeFilm(gmail: number): Promise<void> {
  await http.send("DELETE", `/api/v1/gmails/${gmail}/browser/film`, undefined, ignoreBody);
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
