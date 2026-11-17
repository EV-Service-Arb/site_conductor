import Ajv2020 from "ajv/dist/2020";

import openapi from "@/shared/api/openapi.json";
import type { BrowserClose, BrowserIn, BrowserOut } from "@/shared/api/schema";

const { enum: CODES, "x-enum-varnames": NAMES } = openapi.components.schemas.BrowserClose;

/** The close code the document's `BrowserClose` names so: one it no longer has fails the page as it loads. */
function closed(name: string): BrowserClose {
  const code = CODES[NAMES.indexOf(name)];
  if (code === undefined) throw new Error(`the document's BrowserClose has no ${name}`);
  return code as BrowserClose;
}

/** The server's close codes for the viewer's socket. */
export const CLOSE = { signedOut: closed("SignedOut"), notRunning: closed("NotRunning"), replaced: closed("Replaced"), stopped: closed("Stopped"), goingAway: closed("GoingAway") };

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

/** A text frame's most code points, as the document's `BrowserIn` takes it. */
const TEXT_CHUNK = (() => {
  const typed = openapi.components.schemas.BrowserIn.oneOf.find((v) => v.properties.type.const === "text");
  const max = typed !== undefined && "text" in typed.properties ? typed.properties.text.maxLength : undefined;
  if (max === undefined) throw new Error("the document's BrowserIn has no text frame with a maxLength");
  return max;
})();

/** `text` as the frames that type it, split between code points. */
export function textFrames(text: string): BrowserIn[] {
  const points = Array.from(text);
  const frames: BrowserIn[] = [];
  for (let i = 0; i < points.length; i += TEXT_CHUNK) frames.push({ type: "text", text: points.slice(i, i + TEXT_CHUNK).join("") });
  return frames;
}

/** Keys the page acts on that type nothing; anything printable goes as text. */
export const KEYS = new Set(["Enter", "Backspace", "Tab", "Escape", "Delete", "Home", "End", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"]);
