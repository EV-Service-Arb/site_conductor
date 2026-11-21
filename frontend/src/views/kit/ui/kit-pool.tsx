"use client";

import { useResource } from "@evinvest/api/react";
import { Button, ConfirmDialog, Skeleton, Textarea, cn } from "@evinvest/uikit";
import { MapPin, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { kit as kitKeys, move } from "@/entities/gmb";
import { Refused, api, ok } from "@/shared/api";
import type { Area, Country, KitDraft, KitMember, KitPool as Pool } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { ErrorState } from "@/shared/ui/error-state";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { Chip, Dots, HINT, SECTION_HEAD } from "./parts";

const leastUsed = <X extends { uses: number }>(xs: readonly X[]) => [...xs].sort((a, b) => a.uses - b.uses);

type Filter = { keyword: string } | { place: string } | "untagged" | null;
/** Which of a kit's GBPs each draft and photo went to. */
type Went = Map<number, Set<number>>;

interface Cat {
  trade: string;
  country: Country;
  reload: () => void;
}

/** A category's drafts (13q), one pool every kit and GBP of it draws from; from a kit, dots for its GBPs, filled where each went. Photos are its GBPs' brands'. */
export function KitPool({ trade, country, members = [] }: { trade: string; country: Country; members?: readonly KitMember[] }) {
  const pool = useResource(api, "/categories/{trade}/{country}/kit", { params: { path: { trade, country } } });
  const areas = useResource(api, "/areas");
  const ours = members.filter((m) => m.category?.trade === trade && m.category.country === country);
  const went = useWent(ours, pool.status === "ok" ? pool.data : null);
  if (pool.status === "loading") return <Skeleton className="h-64 w-full" />;
  if (pool.status === "error") return <ErrorState failure={pool.failure} onRetry={pool.reload} />;
  const cat = { trade, country, reload: pool.reload };
  const places = areas.status === "ok" ? areas.data.areas.filter((a) => a.country === country) : [];
  return (
    <div className="@container flex flex-col">
      <Drafts pool={pool.data} cat={cat} places={places} went={went} of={ours.length} />
    </div>
  );
}

function useWent(members: readonly KitMember[], pool: Pool | null): Went {
  const [went, setWent] = useState<Went>(new Map());
  const key = members.map((m) => m.id).join(",");
  const version = pool === null ? "" : pool.drafts.map((d) => d.uses).join(",");
  useEffect(() => {
    if (key === "") return;
    let live = true;
    Promise.all(key.split(",").map(Number).map(async (id) => ({ id, kit: await ok(api.GET("/gmbs/{id}/kit", { params: { path: { id }, query: { since: 0 } } })) }))).then(
      (all) => {
        if (!live) return;
        const next: Went = new Map();
        for (const { id, kit } of all) for (const d of kit.drafts) if (d.went_here) next.set(d.id, (next.get(d.id) ?? new Set()).add(id));
        setWent(next);
      },
      (e: unknown) => notifyFailure(e, () => ""),
    );
    return () => {
      live = false;
    };
  }, [key, version]);
  return went;
}

function Drafts({ pool, cat, places, went, of }: { pool: Pool; cat: Cat; places: readonly Area[]; went: Went; of: number }) {
  const t = useT();
  const button = useButtonSize();
  const [filter, setFilter] = useState<Filter>(null);
  const [open, setOpen] = useState<number | "new" | null>(null);
  const keywords = [...new Set([...pool.drafts.flatMap((d) => d.keywords), ...pool.unmentioned])];
  const placed = [...new Set(pool.drafts.flatMap((d) => d.places))];
  const shown = leastUsed(pool.drafts).filter((d) =>
    filter === null ? true : filter === "untagged" ? d.keywords.length === 0 && d.places.length === 0 : "keyword" in filter ? d.keywords.includes(filter.keyword) : d.places.includes(filter.place),
  );
  const chip = (on: boolean, label: string, pick: () => void, place = false) => (
    <button key={label} type="button" aria-pressed={on} onClick={pick} className={cn("rounded px-2 whitespace-nowrap py-0.5 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring", on ? "bg-ink text-background" : place ? "bg-accent-warn/15 text-accent-warn" : "bg-hover text-ink-soft hover:text-ink")}>
      {label}
    </button>
  );
  const editing = open === "new" ? null : pool.drafts.find((d) => d.id === open);
  return (
    <div className="flex min-h-96 flex-col @3xl:flex-row">
      <section className="flex min-w-0 flex-1 flex-col gap-2 p-5">
        <div className="flex flex-wrap gap-1">
          {chip(filter === null, t(kitKeys.pool.all, "All {n}", { n: pool.drafts.length }), () => setFilter(null))}
          {keywords.map((k) => chip(filter !== null && filter !== "untagged" && "keyword" in filter && filter.keyword === k, `${k} ${pool.drafts.filter((d) => d.keywords.includes(k)).length}`, () => setFilter({ keyword: k })))}
          {placed.map((p) => chip(filter !== null && filter !== "untagged" && "place" in filter && filter.place === p, `${places.find((a) => a.slug === p)?.name ?? p} ${pool.drafts.filter((d) => d.places.includes(p)).length}`, () => setFilter({ place: p }), true))}
          {chip(filter === "untagged", t(kitKeys.pool.untagged, "no tag {n}", { n: pool.drafts.filter((d) => d.keywords.length === 0 && d.places.length === 0).length }), () => setFilter("untagged"))}
        </div>
        <h3 className={SECTION_HEAD}>
          {t(kitKeys.pool.leastUsed, "Least used first")}
          <span className="flex-1" />
          {of > 0 && <span className={HINT}>{t(kitKeys.pool.dots, "dots: this kit's {n} GBPs, filled = went there", { n: of })}</span>}
        </h3>
        {pool.unmentioned.length > 0 && <p className="text-xs text-accent-warn">{t(kitKeys.pool.unmentioned, "no draft mentions: {k}", { k: pool.unmentioned.join(", ") })}</p>}
        {pool.drafts.length === 0 && <p className="text-sm text-ink-soft">{t(kitKeys.drafts.none, "No drafts yet: a reviewer's page offers Google's form alone.")}</p>}
        <ul className="flex flex-col gap-2">
          {shown.map((d) => (
            <li key={d.id}>
              <button type="button" onClick={() => setOpen(d.id)} className={cn("flex w-full items-center gap-3 rounded-lg border p-3 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring", open === d.id ? "border-ink" : "border-border")}>
                <span className="flex min-w-0 flex-1 flex-col gap-1">
                  <span className="line-clamp-2 text-sm text-ink">{d.text}</span>
                  <span className="flex flex-wrap gap-1">
                    {d.keywords.map((k) => (
                      <Chip key={k}>{k}</Chip>
                    ))}
                    {d.places.map((p) => (
                      <Chip key={p} tone="place">
                        <MapPin aria-hidden className="size-3" />
                        {places.find((a) => a.slug === p)?.name ?? p}
                      </Chip>
                    ))}
                  </span>
                </span>
                <span className="flex w-20 shrink-0 flex-col items-end gap-1">
                  <span className="font-mono text-sm text-ink tabular-nums">{t(kitKeys.uses, "{n} uses", { n: d.uses })}</span>
                  <Dots went={went.get(d.id)?.size ?? 0} of={of} />
                </span>
              </button>
            </li>
          ))}
        </ul>
        <Button variant="outline" size={button("sm")} className="self-start" onClick={() => setOpen("new")}>
          <Plus aria-hidden />
          {t(kitKeys.drafts.add, "Add a draft")}
        </Button>
      </section>
      <aside className="border-t border-border p-5 @3xl:w-88 @3xl:shrink-0 @3xl:border-t-0 @3xl:border-l">
        {open === null ? (
          <p className="text-sm text-ink-soft">{t(kitKeys.pool.pick, "Pick a draft to edit it, or add one.")}</p>
        ) : (
          <DraftEditor key={open} cat={cat} draft={editing ?? null} keywords={keywords} places={places} onDone={() => setOpen(null)} />
        )}
      </aside>
    </div>
  );
}

/** A draft's text (while never copied: a use is of the text it had) and its places. */
function DraftEditor({ cat, draft, keywords, places, onDone }: { cat: Cat; draft: KitDraft | null; keywords: readonly string[]; places: readonly Area[]; onDone: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const [text, setText] = useState(draft?.text ?? "");
  const [tagged, setTagged] = useState(draft?.places ?? []);
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const copied = draft !== null && draft.uses > 0;
  const missing = keywords.filter((k) => !(draft?.keywords ?? []).includes(k));
  const path = { trade: cat.trade, country: cat.country };
  const save = async () => {
    setBusy(true);
    try {
      if (draft === null) await ok(api.POST("/categories/{trade}/{country}/kit/drafts", { params: { path }, body: { text: text.trim(), places: tagged } }));
      else await ok(api.PATCH("/categories/{trade}/{country}/kit/drafts/{draft}", { params: { path: { ...path, draft: draft.id } }, body: { ...(text.trim() !== draft.text && { text: text.trim() }), places: tagged } }));
      cat.reload();
      onDone();
    } catch (e) {
      if (!(e instanceof Refused)) throw e;
      if (e.failure.code !== "unauthenticated") setRefused(failureText(e.failure, t));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div className="flex items-baseline gap-2">
        <h3 className={cn(SECTION_HEAD, "flex-1")}>{draft === null ? t(kitKeys.pool.newDraft, "A new draft") : t(kitKeys.pool.draft, "Draft #{id} · {n} uses", { id: draft.id, n: draft.uses })}</h3>
        {draft !== null && <Remove cat={cat} id={draft.id} title={t(kitKeys.drafts.remove, "Remove the draft")} onDone={onDone} />}
      </div>
      <Textarea autoFocus rows={4} value={text} disabled={copied} onChange={(e) => setText(e.target.value)} placeholder={t(kitKeys.drafts.placeholder, "A review as a customer would write it, in the country's language")} aria-label={t(kitKeys.drafts.text, "The draft's text")} />
      <p className="text-xs text-ink-soft">{copied ? t(kitKeys.pool.copied, "copied already: its text stays; add another draft") : t(kitKeys.pool.length, "{n} / 4000", { n: text.length })}</p>
      <div className="flex flex-col gap-1">
        <span className="text-sm text-ink">{t(kitKeys.pool.about, "About")}</span>
        <span className="flex flex-wrap gap-1">{draft !== null && draft.keywords.map((k) => <Chip key={k}>{k}</Chip>)}</span>
        {missing.length > 0 && <span className="text-xs text-ink-soft">{t(kitKeys.pool.misses, "mentions none of: {k}", { k: missing.join(", ") })}</span>}
      </div>
      <div className="flex flex-col gap-1">
        <span className="text-sm text-ink">{t(kitKeys.pool.only, "Only for GBPs in")}</span>
        <span className="flex flex-wrap gap-1">
          {places.length === 0 && <span className="text-xs text-ink-soft">{t(kitKeys.pool.noAreas, "No area of this country drawn yet: a kit's editor draws one on the map.")}</span>}
          {places.map((a) => {
            const on = tagged.includes(a.slug);
            return (
              <button key={a.slug} type="button" aria-pressed={on} onClick={() => setTagged(on ? tagged.filter((x) => x !== a.slug) : [...tagged, a.slug])} className={cn("flex items-center gap-1 rounded px-2 py-0.5 text-xs outline-none", on ? "bg-accent-warn/15 text-accent-warn ring-1 ring-accent-warn" : "bg-hover text-ink-soft hover:text-ink")}>
                <MapPin aria-hidden className="size-3" />
                {a.name}
              </button>
            );
          })}
        </span>
        {tagged.length === 0 && <span className="text-xs text-ink-soft">{t(kitKeys.pool.everywhere, "none: any GBP of the category")}</span>}
        {draft !== null &&
          draft.names.map((n) => (
            <span key={n} className="text-xs text-accent-warn">
              {t(kitKeys.pool.names, "names {n}: offered everywhere", { n })}
            </span>
          ))}
      </div>
      {refused !== null && <p className="text-xs text-accent-error">{refused}</p>}
      <span className="flex justify-end gap-2">
        <Button type="button" variant="ghost" size={button("sm")} onClick={onDone}>
          {t(kitKeys.pool.discard, "Discard")}
        </Button>
        <Button type="submit" size={button("sm")} disabled={busy || text.trim() === ""}>
          {t(kitKeys.save, "Save")}
        </Button>
      </span>
    </form>
  );
}

/** Out of every offer, its uses kept: a removed draft still keeps the GBPs it went to. */
function Remove({ cat, id, title, onDone }: { cat: Cat; id: number; title: string; onDone: () => void }) {
  const t = useT();
  const button = useButtonSize();
  return (
    <ConfirmDialog
      trigger={
        <Button variant="ghost" size={button("sm")} icon aria-label={title} title={title}>
          <Trash2 aria-hidden />
        </Button>
      }
      tone="error"
      title={title}
      description={t(kitKeys.remove.body, "Out of every offer from now on. The GBPs it went to keep it as used.")}
      confirmLabel={t(kitKeys.remove, "Remove")}
      labels={{ cancel: t(move.cancel, "Cancel") }}
      onConfirm={async () => {
        await ok(api.DELETE("/categories/{trade}/{country}/kit/drafts/{draft}", { params: { path: { trade: cat.trade, country: cat.country, draft: id } } }));
        cat.reload();
        onDone();
      }}
    />
  );
}
