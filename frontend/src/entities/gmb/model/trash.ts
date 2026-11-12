import { type Parser, arrayOf, bool, nullable, num, object, oneOf, str } from "@/shared/lib/parse";

import type { Trash, TrashedGmail, TrashedGmb } from "./generated";
import { STATES } from "./gmb";

export type { Trash, TrashSettings, TrashedGmail, TrashedGmb } from "./generated";

const gmailParser: Parser<TrashedGmail> = object({ id: num, gmail: str, removed_at: str, purge_at: nullable(str), gmbs: num, blocked: nullable(str) });
const gmbParser: Parser<TrashedGmb> = object({ id: num, name: str, state: oneOf(STATES), gmail: nullable(str), removed_at: str, purge_at: nullable(str), blocked: nullable(str) });
export const trashParser: Parser<Trash> = object({ settings: object({ keep_days: num, never_purge: bool }), gmails: arrayOf(gmailParser), gmbs: arrayOf(gmbParser) });

