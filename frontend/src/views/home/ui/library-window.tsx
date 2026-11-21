"use client";

import { useResource } from "@evinvest/api/react";
import { Badge, Button, Input, NativeSelect, NativeSelectOption, ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";
import { GitFork, Map as MapIcon } from "lucide-react";
import { type ReactNode, useState } from "react";

import { PublishDialog, VersionDiff, categoryName, googleName, openWindow, trades as tradesKeys, useAdopt, useCategories } from "@/entities/gmb";
import { useMe } from "@/entities/session";
import { setMapView } from "@/features/placement-overlay";
import { api, ok } from "@/shared/api";
import { type Category, type CategoryVersion, type Country, type Kind, type Listing, type ListingVersion, type PublishedStudy, type Sort, countryValues } from "@/shared/api/schema";
import { type T, state, useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const KINDS = ["standard", "community", "mine"] as const satisfies readonly Kind[];
const KIND_NAME = {
  standard: (t) => t(tradesKeys.library.kind.standard, "Standard"),
  community: (t) => t(tradesKeys.library.kind.community, "Community"),
  mine: (t) => t(tradesKeys.library.kind.mine, "Mine"),
} satisfies Record<(typeof KINDS)[number], (t: T) => string>;
const SORT_NAME = {
  rank: (t) => t(tradesKeys.library.sort.rank, "Rank"),
  recent: (t) => t(tradesKeys.library.sort.recent, "Recent"),
  used: (t) => t(tradesKeys.library.sort.used, "Most used"),
} satisfies Record<Sort, (t: T) => string>;

const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";

/** The library: everyone's released category versions as listings, the standard ones curated; a listing's versions, definition, studies and use beside the list. */
export function LibraryWindow({ listing, page }: { listing: number | null; page: boolean }) {
  const t = useT();
  const size = useControlSize();
  const [country, setCountry] = useState<Country | "">("");
  const [kind, setKind] = useState<(typeof KINDS)[number] | "">("");
  const [sort, setSort] = useState<Sort>("rank");
  const [q, setQ] = useState("");
  const query = { sort, ...(country !== "" && { country }), ...(kind !== "" && { kind }), ...(q.trim() !== "" && { q: q.trim() }) };
  const list = useResource(api, "/library", { params: { query }, keep: true });
  return (
    <div className={cn("grid min-h-0 flex-1 divide-x divide-border", listing === null ? "grid-cols-1" : page ? "grid-cols-[minmax(0,26rem)_minmax(0,1fr)]" : "grid-cols-[minmax(0,20rem)_minmax(0,1fr)]")}>
      <section className="flex min-h-0 flex-col">
        <div className="flex flex-col gap-2 border-b border-border p-4">
          <Input size={size} value={q} onChange={(e) => setQ(e.target.value)} placeholder={t(tradesKeys.library.search, "Search titles, abouts and trades")} aria-label={t(tradesKeys.library.search, "Search titles, abouts and trades")} />
          <span className="flex flex-wrap items-center gap-2">
            <NativeSelect size="sm" aria-label={t(tradesKeys.window.country, "Country")} value={country} onChange={(e) => setCountry(e.currentTarget.value as Country | "")}>
              <NativeSelectOption value="">{t(tradesKeys.library.anyCountry, "Every country")}</NativeSelectOption>
              {countryValues.map((c) => (
                <NativeSelectOption key={c} value={c}>
                  {c}
                </NativeSelectOption>
              ))}
            </NativeSelect>
            <ToggleGroup type="single" size="sm" variant="outline" value={kind} onValueChange={(v) => setKind(v as (typeof KINDS)[number] | "")} aria-label={t(tradesKeys.library.kind, "Which listings")}>
              {KINDS.map((k) => (
                <ToggleGroupItem key={k} value={k}>
                  {KIND_NAME[k](t)}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <NativeSelect size="sm" aria-label={t(tradesKeys.library.sort, "Sort")} value={sort} onChange={(e) => setSort(e.currentTarget.value as Sort)}>
              {(["rank", "recent", "used"] as const).map((s) => (
                <NativeSelectOption key={s} value={s}>
                  {SORT_NAME[s](t)}
                </NativeSelectOption>
              ))}
            </NativeSelect>
          </span>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {list.status === "loading" && <p className="p-4 text-sm text-ink-soft">{t(state.loading, "Loading…")}</p>}
          {list.status === "error" && <p className="p-4 text-sm text-accent-error">{failureText(list.failure, t)}</p>}
          {list.status === "ok" && list.data.listings.length === 0 && <p className="p-4 text-sm text-ink-soft">{t(tradesKeys.library.none, "No listing matches.")}</p>}
          {list.status === "ok" && (
            <ul className="flex flex-col divide-y divide-border">
              {list.data.listings.map((l) => (
                <li key={l.id}>
                  <Row listing={l} picked={l.id === listing} />
                </li>
              ))}
            </ul>
          )}
        </div>
      </section>
      {listing !== null && <Detail key={listing} id={listing} />}
    </div>
  );
}

function Row({ listing: l, picked }: { listing: Listing; picked: boolean }) {
  const t = useT();
  const locale = useLocale();
  return (
    <button type="button" aria-pressed={picked} onClick={() => openWindow({ kind: "library", listing: l.id })} className={cn("flex w-full flex-col gap-0.5 px-4 py-2 text-left outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring", picked && "bg-hover")}>
      <span className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-sm font-medium text-ink">{l.title}</span>
        <Badges listing={l} />
      </span>
      <span className="truncate text-xs text-ink-soft">
        {[categoryName(l.trade, l.country, locale), l.author, `v${l.latest.n}`, t(tradesKeys.library.usedBy, "used by {n}", { n: l.used_by })].join(" · ")}
      </span>
    </button>
  );
}

function Badges({ listing: l }: { listing: Listing }) {
  const t = useT();
  return (
    <>
      {l.standard_rank !== null && <Badge tone="positive">{t(tradesKeys.library.ranked, "standard #{rank}", { rank: l.standard_rank })}</Badge>}
      {l.mine && <Badge tone="neutral">{t(tradesKeys.library.kind.mine, "Mine")}</Badge>}
      {l.withdrawn_at !== null && <Badge tone="error">{t(tradesKeys.head.withdrawn, "withdrawn")}</Badge>}
    </>
  );
}

/** A listing: what it is, its actions, its versions as a timeline and a diff of any two, what the one picked says, and the studies published on it. */
function Detail({ id }: { id: number }) {
  const t = useT();
  const read = useResource(api, "/library/{id}", { params: { path: { id } } });
  if (read.status === "loading") return <p className="p-6 text-sm text-ink-soft">{t(state.loading, "Loading…")}</p>;
  if (read.status === "error") return <p className="p-6 text-sm text-accent-error">{failureText(read.failure, t)}</p>;
  return <Listed listing={read.data.listing} versions={read.data.versions} studies={read.data.studies} reload={read.reload} />;
}

function Listed({ listing: l, versions, studies, reload }: { listing: Listing; versions: readonly ListingVersion[]; studies: readonly PublishedStudy[]; reload: () => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const me = useMe();
  const categories = useCategories();
  const adopting = useAdopt();
  const newest = versions.toSorted((a, b) => b.release.n - a.release.n);
  const [shown, setShown] = useState(newest[0]!.release.n);
  const [against, setAgainst] = useState<number | "mine" | null>(newest[1]?.release.n ?? null);
  const [forking, setForking] = useState<Category | null>(null);
  const body = newest.find((v) => v.release.n === shown)!.body;
  const held = categories.status === "ok" ? (categories.data.find((c) => c.trade.id === l.trade.id && c.country === l.country) ?? null) : null;
  const other: CategoryVersion | null = against === "mine" ? (held?.head ?? null) : against === null ? null : newest.find((v) => v.release.n === against)!.body;
  const released = newest.find((v) => v.release.n === shown)!.release;
  const curate = api.mayCall("put", "/library/{id}/standard", me.permissions);
  const act = (action: () => Promise<unknown>) => async () => {
    try {
      await action();
      reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <section className="flex min-h-0 flex-col overflow-y-auto">
      <header className="flex flex-col gap-1 border-b border-border px-6 py-4">
        <h2 className="flex items-center gap-2 text-lg font-semibold text-ink">
          <span className="min-w-0 flex-1 truncate">{l.title}</span>
          <Badges listing={l} />
        </h2>
        <p className="text-sm text-ink-soft">
          <button type="button" onClick={() => openWindow({ kind: "trade", trade: l.trade.id, country: l.country })} className="rounded outline-none hover:text-ink hover:underline focus-visible:ring-2 focus-visible:ring-ring">
            {categoryName(l.trade, l.country, locale)}
          </button>
          {" · "}
          {t(tradesKeys.library.by, "by {author}, {at}", { author: l.author, at: formatDateTime(l.created_at, locale) })}
          {" · "}
          {t(tradesKeys.library.usedBy, "used by {n}", { n: l.used_by })}
        </p>
        {l.forked_from !== null && (
          <button type="button" onClick={() => openWindow({ kind: "library", listing: l.forked_from! })} className="flex items-center gap-1 self-start rounded text-xs text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
            <GitFork aria-hidden className="size-3" />
            {t(tradesKeys.library.forkedFrom, "forked from listing {id}", { id: l.forked_from })}
          </button>
        )}
        {l.about !== "" && <p className="text-sm text-ink">{l.about}</p>}
        <span className="flex flex-wrap items-center gap-2 pt-2">
          <Button size={button("sm")} disabled={categories.status !== "ok"} onClick={() => adopting.adopt(l, released, () => undefined)}>
            {t(tradesKeys.library.useVersion, "Use v{n}", { n: released.n })}
          </Button>
          <Button variant="outline" size={button("sm")} disabled={categories.status !== "ok"} onClick={() => adopting.adopt(l, released, setForking)}>
            <GitFork aria-hidden />
            {t(tradesKeys.library.fork, "Fork")}
          </Button>
          {held !== null && (
            <Button variant="outline" size={button("sm")} aria-pressed={against === "mine"} onClick={() => setAgainst(against === "mine" ? null : "mine")}>
              {t(tradesKeys.library.compare, "Compare with mine")}
            </Button>
          )}
          <span className="flex-1" />
          {l.mine && l.withdrawn_at === null && (
            <Button variant="ghost" size={button("sm")} onClick={() => void act(() => ok(api.POST("/library/{id}/withdraw", { params: { path: { id: l.id } } })))()}>
              {t(tradesKeys.library.withdraw, "Withdraw")}
            </Button>
          )}
          {curate && <Curate listing={l} onSet={(rank) => void act(() => ok(api.PUT("/library/{id}/standard", { params: { path: { id: l.id } }, body: { rank } })))()} />}
        </span>
      </header>
      <div className="grid gap-6 p-6 @container">
        <Part title={t(tradesKeys.library.versions, "Versions")}>
          <ol className="flex flex-col divide-y divide-border">
            {newest.map(({ release: r }) => (
              <li key={r.id} className="flex items-center gap-3 py-1.5 text-sm">
                <button type="button" aria-pressed={r.n === shown} onClick={() => setShown(r.n)} className={cn("w-12 shrink-0 rounded font-mono text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring", r.n === shown ? "font-semibold text-ink" : "text-ink-soft hover:text-ink")}>
                  v{r.n}
                </button>
                <span className="min-w-0 flex-1 truncate text-ink">{r.notes === "" ? <span className="text-ink-soft">{t(tradesKeys.library.noNotes, "no notes")}</span> : r.notes}</span>
                <span className="text-xs text-ink-soft">{formatDateTime(r.published_at, locale)}</span>
                {r.n !== shown && (
                  <Button variant="ghost" size={button("sm")} aria-pressed={against === r.n} onClick={() => setAgainst(against === r.n ? null : r.n)}>
                    {t(tradesKeys.library.diff, "diff")}
                  </Button>
                )}
              </li>
            ))}
          </ol>
          {other !== null && (
            <div className="flex flex-col gap-1">
              <span className="text-xs text-ink-soft">{against === "mine" ? t(tradesKeys.library.fromMine, "v{n} against your head", { n: shown }) : t(tradesKeys.library.from, "v{n} against v{m}", { n: shown, m: String(against) })}</span>
              <VersionDiff from={other} to={body} />
            </div>
          )}
        </Part>
        <Part title={t(tradesKeys.library.definition, "Definition · v{n}", { n: shown })}>
          <Definition body={body} />
        </Part>
        <Part title={t(tradesKeys.library.studies, "Studies · {n}", { n: studies.length })}>
          {studies.length === 0 ? (
            <p className="text-sm text-ink-soft">{t(tradesKeys.library.noStudies, "No study published on it yet.")}</p>
          ) : (
            <ul className="flex flex-col divide-y divide-border">
              {studies.map((s) => (
                <li key={s.sha} className="flex items-center gap-3 py-1.5 text-sm">
                  <span className="min-w-0 flex-1 truncate text-ink">
                    {s.title} <span className="text-xs text-ink-soft">v{s.release.n} · {s.author}</span>
                  </span>
                  {s.withdrawn_at !== null && <Badge tone="error">{t(tradesKeys.head.withdrawn, "withdrawn")}</Badge>}
                  <Button variant="ghost" size={button("sm")} onClick={() => setMapView({ area: s.area, study: { kind: "published", sha: s.sha } })}>
                    <MapIcon aria-hidden />
                    {t(tradesKeys.overview.areas.map, "Map")}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </Part>
        <Part title={t(tradesKeys.library.usedByTitle, "Used by")}>
          <p className="text-sm text-ink">{t(tradesKeys.library.usedByCount, "{n} people have their category on one of its versions.", { n: l.used_by })}</p>
        </Part>
      </div>
      {adopting.dialog}
      {forking !== null && <PublishDialog category={forking} fork={l} onClose={() => setForking(null)} />}
    </section>
  );
}

function Part({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className={SECTION_HEAD}>{title}</h3>
      {children}
    </section>
  );
}

/** What a version says, read-only. */
function Definition({ body }: { body: CategoryVersion }) {
  const t = useT();
  const locale = useLocale();
  const line = "flex items-baseline gap-2 text-sm";
  return (
    <dl className="flex flex-col gap-2">
      <div className={line}>
        <dt className="w-28 shrink-0 text-xs text-ink-soft">{t(tradesKeys.google.categories, "Google categories")}</dt>
        <dd className="text-ink">{[googleName(body.trade, locale), ...body.additional.map((a) => `${googleName(a.google, locale)} ×${a.weight}`)].join(" › ")}</dd>
      </div>
      <div className={line}>
        <dt className="w-28 shrink-0 text-xs text-ink-soft">{t(tradesKeys.keywords.weight, "Keywords · weight")}</dt>
        <dd className="text-ink">{body.keywords.map((k) => (k.weight === null ? k.text : `${k.text} ×${k.weight}`)).join(" · ")}</dd>
      </div>
      {(body.excluded.words.length > 0 || body.excluded.types.length > 0) && (
        <div className={line}>
          <dt className="w-28 shrink-0 text-xs text-ink-soft">{t(tradesKeys.excluded, "Excluded")}</dt>
          <dd className="text-ink">{[...body.excluded.words, ...body.excluded.types.map((x) => `[${x}]`)].join(" · ")}</dd>
        </div>
      )}
      <div className={line}>
        <dt className="w-28 shrink-0 text-xs text-ink-soft">{t(tradesKeys.demand, "Demand")}</dt>
        <dd className="font-mono text-xs text-ink">{body.demand === null ? t(tradesKeys.demand.none, "No demand surface.") : body.demand.expr}</dd>
      </div>
    </dl>
  );
}

/** In the standard library at a rank, or out of it: a curator's say. */
function Curate({ listing, onSet }: { listing: Listing; onSet: (rank: number | null) => void }) {
  const t = useT();
  const button = useButtonSize();
  const [rank, setRank] = useState(listing.standard_rank === null ? "" : String(listing.standard_rank));
  const n = Number(rank);
  return (
    <span className="flex items-center gap-1">
      <Input size="sm" className="w-16" inputMode="numeric" aria-label={t(tradesKeys.library.rank, "Standard rank")} placeholder={t(tradesKeys.library.rank.short, "rank")} value={rank} onChange={(e) => setRank(e.target.value)} />
      <Button variant="outline" size={button("sm")} disabled={rank === "" || !Number.isSafeInteger(n) || n === listing.standard_rank} onClick={() => onSet(n)}>
        {t(tradesKeys.library.rank.set, "Make standard")}
      </Button>
      {listing.standard_rank !== null && (
        <Button variant="ghost" size={button("sm")} onClick={() => onSet(null)}>
          {t(tradesKeys.library.rank.drop, "Out of the standard")}
        </Button>
      )}
    </span>
  );
}
