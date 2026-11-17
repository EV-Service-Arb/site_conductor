"use client";

import { Button, Card, CardContent, InputGroup, InputGroupAddon, InputGroupInput, Table, TableBody, TableCell, TableHead, TableHeader, TableRow, cn } from "@evinvest/uikit";
import { ArrowDown, ArrowUp, ArrowUpDown, Search } from "lucide-react";
import { type ReactNode, useState } from "react";

import { tokens } from "@/entities/tokens";
import type { TokenHolder } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { age } from "@/shared/lib/time";
import { EDGE_CELL, TABLE_HEAD } from "@/shared/ui/table";
import { useButtonSize } from "@/shared/ui/touch";

const HEAD = cn(TABLE_HEAD, EDGE_CELL);

type Key = "person" | "balance" | "age";
type Sort = { key: Key; desc: boolean };

const label = (h: TokenHolder) => h.name || h.email;
const COMPARE: Record<Key, (a: TokenHolder, b: TokenHolder) => number> = {
  person: (a, b) => label(a).localeCompare(label(b)),
  balance: (a, b) => a.balance - b.balance,
  age: (a, b) => Date.parse(b.first_seen) - Date.parse(a.first_seen), // ascending: the newest first
};

/** Everyone's balance, searched by name or email, sorted by any column. */
export function HoldersCard({ holders, onChange }: { holders: TokenHolder[]; onChange: (h: TokenHolder) => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>({ key: "person", desc: false });

  const q = query.trim().toLowerCase();
  const rows = holders.filter((h) => h.name.toLowerCase().includes(q) || h.email.toLowerCase().includes(q)).sort((a, b) => (sort.desc ? -1 : 1) * COMPARE[sort.key](a, b));

  const head = (key: Key, children: ReactNode, right?: boolean) => {
    const on = sort.key === key;
    const Icon = !on ? ArrowUpDown : sort.desc ? ArrowDown : ArrowUp;
    return (
      <TableHead className={cn(HEAD, right && "text-right")} aria-sort={on ? (sort.desc ? "descending" : "ascending") : "none"}>
        <button type="button" className={cn("inline-flex items-center gap-1 uppercase hover:text-ink", on && "text-ink")} onClick={() => setSort({ key, desc: on ? !sort.desc : key === "balance" })}>
          {children}
          <Icon aria-hidden className={cn("size-3", !on && "opacity-40")} />
        </button>
      </TableHead>
    );
  };

  return (
    <Card className="gap-0 overflow-hidden py-0">
      <div className="border-b border-border p-3">
        <InputGroup>
          <InputGroupAddon>
            <Search aria-hidden />
          </InputGroupAddon>
          <InputGroupInput type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t(tokens.admin.search, "Name or email")} aria-label={t(tokens.admin.search, "Name or email")} />
        </InputGroup>
      </div>
      {/* the kit's container is the table's scroller, so the head sticks in it */}
      <CardContent className="p-0 [&_[data-slot=table-container]]:max-h-[60vh] [&_[data-slot=table-container]]:overflow-y-auto">
        <Table>
          <TableHeader className="sticky top-0 z-10 bg-card">
            <TableRow className="hover:bg-transparent">
              {head("person", t(tokens.admin.user, "Person"))}
              {head("balance", t(tokens.admin.balance, "Tokens"), true)}
              {head("age", t(tokens.admin.age, "Age"), true)}
              <TableHead className={HEAD}>
                <span className="sr-only">{t(tokens.admin.change, "Change")}</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((h) => (
              <TableRow key={h.user_id}>
                <TableCell className={EDGE_CELL}>
                  <span className="text-ink">{label(h)}</span>
                  {h.name && <span className="ml-2 text-ink-soft">{h.email}</span>}
                </TableCell>
                <TableCell className={cn(EDGE_CELL, "text-right tabular-nums")}>{h.balance}</TableCell>
                <TableCell className={cn(EDGE_CELL, "text-right tabular-nums text-ink-soft")} title={h.first_seen}>
                  {age(h.first_seen, locale)}
                </TableCell>
                <TableCell className={cn(EDGE_CELL, "text-right")}>
                  <Button variant="ghost" size={button("sm")} onClick={() => onChange(h)}>
                    {t(tokens.admin.change, "Change")}
                  </Button>
                </TableCell>
              </TableRow>
            ))}
            {rows.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={4} className={cn(EDGE_CELL, "text-center text-ink-soft")}>
                  {t(tokens.admin.noMatch, "Nobody matches.")}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
}
