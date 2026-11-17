import Ajv2020 from "ajv/dist/2020";

import openapi from "@/shared/api/openapi.json";
import type { BrowserIn, BrowserOut } from "@/shared/api/schema";

/** The server's close codes for the viewer's socket. */
export const CLOSE = { signedOut: 4401, notRunning: 4404, takenOver: 4409, stopped: 4410, goingAway: 1001 } as const;

const ajv = new Ajv2020({ strict: false, validateFormats: false, validateSchema: false });
ajv.addSchema(openapi, "api");
const OUT = (() => {
  const validate = ajv.getSchema<BrowserOut>("api#/components/schemas/BrowserOut");
  if (validate === undefined) throw new Error("the document's BrowserOut does not compile");
  return validate;
})();

/** A viewer frame, checked against the document's `BrowserOut`. */
export function parseOut(text: string): BrowserOut {
  const v: unknown = JSON.parse(text);
  if (!OUT(v)) throw new Error(`browser socket: ${ajv.errorsText(OUT.errors)}`);
  return v as BrowserOut;
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
