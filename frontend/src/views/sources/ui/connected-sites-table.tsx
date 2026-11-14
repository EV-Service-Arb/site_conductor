"use client";

import { Badge, Card, CardContent, Table, TableBody, TableCell, TableHead, TableHeader, TableRow, cn } from "@evinvest/uikit";

import type { ConnectedSite, SourceState } from "@/entities/source";
import { type T, useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { EDGE_CELL, TABLE_HEAD } from "@/shared/ui/table";
import { connected } from "../model/i18n";
import { sources } from "@/entities/source";

const HEAD = cn(TABLE_HEAD, EDGE_CELL);

const STATE_TONE = { unset: "neutral", live: "positive", empty: "neutral", withdrawn: "neutral", down: "error" } as const;

/** Every active site key: what its landing says of itself, beside what sites saw of it. */
export function ConnectedSitesTable({ sites }: { sites: ConnectedSite[] }) {
  const t = useT();
  return (
    <Card className="overflow-hidden py-0">
      <CardContent className="overflow-x-auto p-0">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className={HEAD}>{t(sources.keyId, "Key id")}</TableHead>
              <TableHead className={HEAD}>{t(sources.brands, "Brands")}</TableHead>
              <TableHead className={HEAD}>{t(sources.origin, "Origin")}</TableHead>
              <TableHead className={HEAD}>{t(sources.state, "State")}</TableHead>
              <TableHead className={HEAD}>kitstart</TableHead>
              <TableHead className={cn(HEAD, "text-right")}>{t(connected.lastIngest, "Last event")}</TableHead>
              <TableHead className={cn(HEAD, "text-right")}>{t(connected.lastRead, "Last data read")}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {sites.map((s) => (
              <Row key={s.keyId} site={s} />
            ))}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
}

function Row({ site }: { site: ConnectedSite }) {
  const t = useT();
  const locale = useLocale();
  const seen = (at: string | null | undefined) => (at === undefined ? t(connected.unknown, "unknown") : at === null ? t(connected.never, "never") : formatDateTime(at, locale));
  const readsTitle = site.reads.map((r) => `${r.brand} ${r.endpoint}: ${formatDateTime(r.at, locale)}`).join("\n");
  return (
    <TableRow>
      <TableCell className={cn(EDGE_CELL, "font-mono text-xs")}>{site.keyId}</TableCell>
      <TableCell className={cn(EDGE_CELL, "text-ink-mid")}>{site.brands.join(", ")}</TableCell>
      <TableCell className={cn(EDGE_CELL, "font-mono text-xs")}>{site.origin ?? "—"}</TableCell>
      <TableCell className={EDGE_CELL}>
        <Report site={site} />
      </TableCell>
      <TableCell className={cn(EDGE_CELL, "tabular-nums text-ink-soft")}>{site.report.state === "reported" ? site.report.report.kitstart : "—"}</TableCell>
      <TableCell className={cn(EDGE_CELL, "text-right tabular-nums text-ink-soft")}>{seen(site.lastIngest)}</TableCell>
      <TableCell className={cn(EDGE_CELL, "text-right tabular-nums text-ink-soft")} title={readsTitle || undefined}>
        {seen(site.lastRead)}
      </TableCell>
    </TableRow>
  );
}

function Report({ site }: { site: ConnectedSite }) {
  const t = useT();
  const { report } = site;
  if (report.state === "no_origin") return <Badge tone="neutral">{t(connected.noOrigin, "No origin set")}</Badge>;
  if (report.state === "failed")
    return (
      <Badge tone="error" title={report.detail}>
        {t(connected.failed, "No report")}
      </Badge>
    );
  const { places, pricing, experiments } = report.report.sources;
  return (
    <div className="flex flex-wrap gap-1">
      {Object.entries(places).map(([slug, s]) => (
        <State key={slug} label={slug} state={s} />
      ))}
      <State label={t(connected.pricing, "pricing")} state={pricing} />
      <State label={t(connected.experiments, "experiments")} state={experiments} />
    </div>
  );
}

const STATE_NAME = {
  unset: (t) => t(connected.state.unset, "not configured"),
  live: (t) => t(connected.state.live, "live"),
  empty: (t) => t(connected.state.empty, "empty"),
  withdrawn: (t) => t(connected.state.withdrawn, "withdrawn"),
  down: (t) => t(connected.state.down, "down"),
} satisfies Record<SourceState["state"], (t: T) => string>;

function State({ label, state }: { label: string; state: SourceState }) {
  const t = useT();
  return (
    <Badge tone={STATE_TONE[state.state]} title={state.detail ?? undefined}>
      {label}: {STATE_NAME[state.state](t)}
    </Badge>
  );
}
