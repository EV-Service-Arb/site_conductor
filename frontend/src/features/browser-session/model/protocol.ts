import type { BrowserIn, BrowserOut } from "@/entities/gmb";
import { type Parser, arrayOf, nullable, num, object, oneOf, parse, str } from "@/shared/lib/parse";

/** The server's close codes for the viewer's socket. */
export const CLOSE = { signedOut: 4401, notRunning: 4404, takenOver: 4409, stopped: 4410, goingAway: 1001 } as const;

const OUT: { [K in BrowserOut["type"]]: Parser<Extract<BrowserOut, { type: K }>> } = {
  frame: object({ type: oneOf(["frame"]), data: str, width: num, height: num }),
  tabs: object({ type: oneOf(["tabs"]), tabs: arrayOf(object({ id: str, title: str, url: str })), active: nullable(str) }),
  dialog: object({ type: oneOf(["dialog"]), kind: str, message: str, default: nullable(str) }),
  dialog_closed: object({ type: oneOf(["dialog_closed"]) }),
  error: object({ type: oneOf(["error"]), message: str }),
};

export function parseOut(text: string): BrowserOut {
  const v: unknown = JSON.parse(text);
  const type = typeof v === "object" && v !== null && "type" in v ? v.type : undefined;
  if (typeof type !== "string" || !(type in OUT)) throw new Error(`browser socket: unknown frame ${String(type)}`);
  return parse(OUT[type as BrowserOut["type"]] as Parser<BrowserOut>, v);
}

/** A client frame is at most 4 KiB; a code point is at most 6 bytes of JSON (`\u` escapes), so 600 stay under it with the envelope. */
const TEXT_CHUNK = 600;

/** `text` as the frames that type it, split between code points. */
export function textFrames(text: string): BrowserIn[] {
  const points = Array.from(text);
  const frames: BrowserIn[] = [];
  for (let i = 0; i < points.length; i += TEXT_CHUNK) frames.push({ type: "text", text: points.slice(i, i + TEXT_CHUNK).join("") });
  return frames;
}

/** Keys the page acts on that type nothing; anything printable goes as text. */
export const KEYS = new Set(["Enter", "Backspace", "Tab", "Escape", "Delete", "Home", "End", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"]);
