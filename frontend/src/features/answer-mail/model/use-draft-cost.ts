"use client";

import { useTokens } from "@/entities/tokens";
import { useOwnModels } from "@/features/connect-model";

/** The writer's instructions and what it is told of the GBP; its thinking and the reply. */
const INSTRUCTIONS = 2_000;
const ANSWER = 2_000;
const CHARS_PER_TOKEN = 4;
const PER = 1_000_000;

/** What writing a reply to `texts` costs, about; `undefined`, not priced. `null` while it is not known on whose models it runs. */
export function useDraftCost(texts: readonly string[]): { own: true } | { own: false; cost: number | undefined } | null {
  const mine = useOwnModels();
  const held = useTokens();
  if (mine === null || held.status !== "ok") return null;
  if (!mine.none) return { own: true };
  const [input, output] = [held.data.prices["claude.input"], held.data.prices["claude.output"]];
  if (input === undefined || output === undefined) return { own: false, cost: undefined };
  const read = INSTRUCTIONS + texts.reduce((n, text) => n + text.length, 0) / CHARS_PER_TOKEN;
  return { own: false, cost: Math.ceil((read * input + ANSWER * output) / PER) };
}
