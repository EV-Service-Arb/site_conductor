import { type Parser, arrayOf, isoDay, nullable, num, object, oneOf, str } from "@/shared/lib/parse";

import { type Balance, type Kickback, LEDGER_KINDS, type Ledger, type LedgerEntry, PRICED_ITEMS, type Price, type Prices, SCOPES, type TokenHolders, type Tokens, type Usage } from "./generated";

export type { BalanceChange as Change, Kickback, LedgerEntry, Price, PricedItem, Scope, SetPrices, TokenHolder as Holder, Tokens, UsageDay } from "./generated";
export { LEDGER_KINDS };

const kickbackParser: Parser<Kickback> = object({
  id: num,
  at: str,
  amount: num,
  service: nullable(str),
  key: nullable(str),
  label: nullable(str),
  by: nullable(str),
  note: nullable(str),
});

export const tokensParser: Parser<Tokens> = object({ balance: num, daily: num, cap: num, kickbacks: arrayOf(kickbackParser) });

const entryParser: Parser<LedgerEntry> = object({
  id: num,
  at: str,
  delta: num,
  kind: oneOf(LEDGER_KINDS),
  service: nullable(str),
  key: nullable(str),
  label: nullable(str),
  by: nullable(str),
  note: nullable(str),
});
export const ledgerParser: Parser<Ledger> = object({ entries: arrayOf(entryParser), next: nullable(num) });

export const usageParser: Parser<Usage> = object({ days: arrayOf(object({ day: isoDay, charges: num, tokens: num })) });

export const holdersParser: Parser<TokenHolders> = object({ users: arrayOf(object({ user_id: str, email: str, name: str, balance: num })) });

export const balanceParser: Parser<Balance> = object({ balance: num });

export const priceParser: Parser<Price> = object({
  item: oneOf(PRICED_ITEMS),
  scope: oneOf(SCOPES),
  tool: str,
  tool_title: str,
  title: str,
  description: str,
  tokens: nullable(num),
  set_by: nullable(str),
  set_at: nullable(str),
});

export const pricesParser: Parser<Prices> = object({ prices: arrayOf(priceParser) });
