"use client";

import { useTokens } from "@/entities/tokens";
import { useClaude } from "@/features/connect-claude";

/** The writer's instructions and what it is told of the GBP; its thinking and the reply. */
const INSTRUCTIONS = 2_000;
const ANSWER = 2_000;
const CHARS_PER_TOKEN = 4;
const PER = 1_000_000;

/** What writing a reply to `texts` costs, about; `undefined`, not priced. `null` while it is not known on whose Claude it runs. */
export function useDraftCost(texts: readonly string[]): { own: true } | { own: false; cost: number | undefined } | null {
  const mine = useClaude();
  const held = useTokens();
  if (mine.status !== "ok" || held.status !== "ok") return null;
  if (mine.data !== null) return { own: true };
  const [input, output] = [held.data.prices["claude.input"], held.data.prices["claude.output"]];
  if (input === undefined || output === undefined) return { own: false, cost: undefined };
  const read = INSTRUCTIONS + texts.reduce((n, text) => n + text.length, 0) / CHARS_PER_TOKEN;
  return { own: false, cost: Math.ceil((read * input + ANSWER * output) / PER) };
}
