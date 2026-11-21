import type { CategoryVersion, Demand, GoogleCategory } from "@/shared/api/schema";

/** What a body is made of, piece by piece. */
export type Piece = "keyword" | "additional" | "order" | "word" | "type" | "demand";

/** One piece apart between two bodies: `from` null where added, `to` null where removed. */
export interface Change {
  piece: Piece;
  key: string;
  google?: GoogleCategory;
  from: string | null;
  to: string | null;
}

type Body = Pick<CategoryVersion, "keywords" | "additional" | "excluded" | "demand">;

const weight = (w: number | null) => (w === null ? "—" : String(w));

function keyed<V>(piece: Piece, a: ReadonlyMap<string, V>, b: ReadonlyMap<string, V>, text: (v: V) => string, google?: (key: string) => GoogleCategory): Change[] {
  const out: Change[] = [];
  for (const [key, v] of a) {
    const w = b.get(key);
    const change = { piece, key, ...(google !== undefined && { google: google(key) }) };
    if (w === undefined) out.push({ ...change, from: text(v), to: null });
    else if (text(v) !== text(w)) out.push({ ...change, from: text(v), to: text(w) });
  }
  for (const [key, w] of b) if (!a.has(key)) out.push({ piece, key, ...(google !== undefined && { google: google(key) }), from: null, to: text(w) });
  return out;
}

const demandText = (d: Demand | null) => (d === null ? null : [d.expr, ...d.columns.map((c) => `${c.name} = ${c.expr}`), ...d.layers.map((l) => `${l.name}: ${l.expr} (${l.scale})`)].join(" · "));

/** How body `b` differs from `a`; empty when they say the same. */
export function diffBodies(a: Body, b: Body): Change[] {
  const googles = new Map([...a.additional, ...b.additional].map((x) => [x.google.id, x.google]));
  const set = (xs: readonly string[]) => new Map(xs.map((x) => [x, x]));
  const shared = (x: readonly { google: GoogleCategory }[], y: readonly { google: GoogleCategory }[]) => x.map((t) => t.google.id).filter((id) => y.some((u) => u.google.id === id));
  const order = shared(a.additional, b.additional).join() !== shared(b.additional, a.additional).join();
  const [da, db] = [demandText(a.demand), demandText(b.demand)];
  return [
    ...keyed("keyword", new Map(a.keywords.map((k) => [k.text, k.weight])), new Map(b.keywords.map((k) => [k.text, k.weight])), weight),
    ...keyed("additional", new Map(a.additional.map((x) => [x.google.id, x.weight])), new Map(b.additional.map((x) => [x.google.id, x.weight])), weight, (id) => googles.get(id)!),
    ...(order ? [{ piece: "order" as const, key: "", from: a.additional.map((x) => x.google.id).join(" "), to: b.additional.map((x) => x.google.id).join(" ") }] : []),
    ...keyed("word", set(a.excluded.words), set(b.excluded.words), () => ""),
    ...keyed("type", set(a.excluded.types), set(b.excluded.types), () => ""),
    ...(da !== db ? [{ piece: "demand" as const, key: "", from: da, to: db }] : []),
  ];
}
