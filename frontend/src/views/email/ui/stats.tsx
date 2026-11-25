"use client";

import { Skeleton, Table, TableBody, TableCell, TableHead, TableHeader, TableRow, cn } from "@evinvest/uikit";
import { useState } from "react";

import { under, useGmails, useScope } from "@/entities/gmb";
import { GOAL_NAME, KIND_NAME, daysOf, figures, mail, useStats, wordings } from "@/entities/mail";
import { type MailGoal, type MailPursuit, mailGoalValues } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";

/** What was asked of Google per gmail and GBP, how much it took, and which wording worked. */
export function Stats() {
  const t = useT();
  const read = useStats();
  const gmails = useGmails();
  const scope = useScope();
  if (read.status === "error") return <ErrorState failure={read.failure} onRetry={read.reload} />;
  if (read.status === "loading" || gmails.status !== "ok") return <Skeleton className="h-60" />;
  const pursuits = read.data.pursuits.filter((p) => scope.gmails === null || scope.gmails.has(p.gmail_id));
  if (pursuits.length === 0) return <EmptyState title={t(mail.stats.none, "Nothing was asked of Google yet")} />;
  return (
    <div className="flex flex-col gap-6">
      {mailGoalValues
        .map((goal) => [goal, pursuits.filter((p) => p.goal === goal)] as const)
        .filter(([, of]) => of.length > 0)
        .map(([goal, of]) => (
          <Goal key={goal} goal={goal} pursuits={of} />
        ))}
    </div>
  );
}

function Goal({ goal, pursuits }: { goal: MailGoal; pursuits: MailPursuit[] }) {
  const t = useT();
  const gmails = useGmails();
  const [now] = useState(() => Date.now());
  if (gmails.status !== "ok") throw new Error("read before its goals are drawn");
  const listed = gmails.data.gmails.filter((g) => pursuits.some((p) => p.gmail_id === g.id));
  const row = (key: string, name: string, of: MailPursuit[], depth: 0 | 1 | 2) => {
    const f = figures(of, now);
    return (
      <TableRow key={key}>
        <TableCell className={cn("max-w-72 truncate", depth === 0 && "font-semibold", depth === 1 && "font-medium", depth === 2 && "pl-8")}>{name}</TableCell>
        <TableCell className="text-right tabular-nums">{t(mail.stats.givenOf, "{given} of {of}", { given: f.given, of: f.of })}</TableCell>
        <TableCell className="text-right tabular-nums">{f.asks ?? "–"}</TableCell>
        <TableCell className="text-right tabular-nums">{f.days ?? "–"}</TableCell>
        <TableCell className="text-right text-ink-soft tabular-nums">{depth < 2 ? f.open.length || "" : f.open.map((p) => t(mail.stats.open, "{n, plural, one {# ask} other {# asks}}, {days} d", { n: p.asks.length, days: daysOf(p, now) })).join(" · ")}</TableCell>
      </TableRow>
    );
  };
  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-sm font-semibold text-ink">{GOAL_NAME[goal](t)}</h2>
      <div className="overflow-x-auto rounded-lg border border-border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead />
              <TableHead className="text-right">{t(mail.stats.given, "Given")}</TableHead>
              <TableHead className="text-right" title={t(mail.stats.median, "Median, of those given")}>
                {t(mail.stats.asks, "Asks until")}
              </TableHead>
              <TableHead className="text-right" title={t(mail.stats.median, "Median, of those given")}>
                {t(mail.stats.days, "Days until")}
              </TableHead>
              <TableHead className="text-right">{t(mail.stats.asking, "Asking now")}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {listed.length > 1 && row("all", t(mail.stats.all, "All"), pursuits, 0)}
            {listed.flatMap((g) => {
              const own = pursuits.filter((p) => p.gmail_id === g.id);
              const gmbs = [...new Set(own.map((p) => p.gmb_id))];
              const names = under(gmails.data, g.id);
              return [
                row(`g${g.id}`, g.gmail, own, 1),
                ...gmbs.map((id) => row(`b${g.id}.${id}`, id === null ? t(mail.gbp.unknown, "GBP not known") : (names.find((b) => b.id === id) ?? gmails.data.gmbs.find((b) => b.id === id))?.name ?? t(mail.stats.removed, "A removed GBP"), own.filter((p) => p.gmb_id === id), 2)),
              ];
            })}
          </TableBody>
        </Table>
      </div>
      <h3 className="text-xs font-medium text-ink-soft">{t(mail.stats.wording, "What we wrote, and what Google answered")}</h3>
      <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
        {wordings(pursuits, gmails.data.gmbs.map((b) => b.name)).map((w) => (
          <li key={w.text} className="flex flex-col gap-1 px-4 py-3 text-sm @2xl:flex-row @2xl:gap-4">
            <span className="flex shrink-0 flex-col @2xl:w-56">
              <span className={cn("tabular-nums", w.worked > 0 ? "text-positive" : "text-ink-soft")}>{t(mail.stats.worked, "worked {worked} of {used}", { worked: w.worked, used: w.used })}</span>
              <span className="text-xs text-ink-soft">{w.answers.map(([k, n]) => `${k === null ? t(mail.stats.silence, "No answer") : KIND_NAME[k](t)}${n > 1 ? ` ×${n}` : ""}`).join(" · ")}</span>
            </span>
            <details className="min-w-0 flex-1">
              <summary className="line-clamp-2 cursor-pointer break-words text-ink outline-none focus-visible:ring-2 focus-visible:ring-ring">{w.text}</summary>
              <p className="mt-2 break-words whitespace-pre-wrap text-ink">{w.text}</p>
            </details>
          </li>
        ))}
      </ul>
    </section>
  );
}
