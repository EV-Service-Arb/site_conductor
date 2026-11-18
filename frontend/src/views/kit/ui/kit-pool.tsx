"use client";

import { useResource } from "@evinvest/api/react";
import { Button, ConfirmDialog, Input, Skeleton, Textarea, cn, toast } from "@evinvest/uikit";
import { ImagePlus, MapPin, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { kit as kitKeys, move, photoMax, uploadPhoto } from "@/entities/gmb";
import { Refused, api, ok } from "@/shared/api";
import type { Area, Country, KitDraft, KitMember, KitPhoto, KitPool as Pool } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { ErrorState } from "@/shared/ui/error-state";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { Chip, Dots, HINT, SECTION_HEAD, Thumb } from "./parts";

const leastUsed = <X extends { uses: number }>(xs: readonly X[]) => [...xs].sort((a, b) => a.uses - b.uses);

type Filter = { keyword: string } | { place: string } | "untagged" | null;
/** Which of a kit's GBPs each draft and photo went to. */
type Went = { drafts: Map<number, Set<number>>; photos: Map<number, Set<number>> };

interface Cat {
  trade: number;
  country: Country;
  reload: () => void;
}

/** A category's drafts and photos (13q, 13r), one pool every kit and GBP of it draws from; from a kit, dots for its GBPs, filled where each went. */
export function KitPool({ trade, country, members = [] }: { trade: number; country: Country; members?: readonly KitMember[] }) {
  const t = useT();
  const pool = useResource(api, "/trades/{id}/{country}/kit", { params: { path: { id: trade, country } } });
  const areas = useResource(api, "/areas");
  const [tab, setTab] = useState<"drafts" | "photos">("drafts");
  const ours = members.filter((m) => m.category?.trade === trade && m.category.country === country);
  const went = useWent(ours, pool.status === "ok" ? pool.data : null);
  if (pool.status === "loading") return <Skeleton className="h-64 w-full" />;
  if (pool.status === "error") return <ErrorState failure={pool.failure} onRetry={pool.reload} />;
  const cat = { trade, country, reload: pool.reload };
  const places = areas.status === "ok" ? areas.data.areas.filter((a) => a.country === country) : [];
  return (
    <div className="flex flex-col">
      <nav role="tablist" className="flex gap-1 border-b border-border px-5">
        {(["drafts", "photos"] as const).map((k) => (
          <button key={k} type="button" role="tab" aria-selected={tab === k} onClick={() => setTab(k)} className={cn("-mb-px border-b-2 px-3 py-2 text-sm outline-none", tab === k ? "border-ink text-ink" : "border-transparent text-ink-soft hover:text-ink")}>
            {k === "drafts" ? t(kitKeys.pool.drafts, "Drafts {n}", { n: pool.data.drafts.length }) : t(kitKeys.pool.photos, "Photos {n}", { n: pool.data.photos.length })}
          </button>
        ))}
      </nav>
      {tab === "drafts" ? <Drafts pool={pool.data} cat={cat} places={places} went={went} of={ours.length} /> : <Photos pool={pool.data} cat={cat} places={places} went={went} members={ours} />}
    </div>
  );
}

function useWent(members: readonly KitMember[], pool: Pool | null): Went {
  const [went, setWent] = useState<Went>({ drafts: new Map(), photos: new Map() });
  const key = members.map((m) => m.id).join(",");
  const version = pool === null ? "" : `${pool.drafts.map((d) => d.uses).join(",")}/${pool.photos.map((p) => p.uses).join(",")}`;
  useEffect(() => {
    if (key === "") return;
    let live = true;
    Promise.all(key.split(",").map(Number).map(async (id) => ({ id, kit: await ok(api.GET("/gmbs/{id}/kit", { params: { path: { id }, query: { since: 0 } } })) }))).then(
      (all) => {
        if (!live) return;
        const next: Went = { drafts: new Map(), photos: new Map() };
        const add = (m: Map<number, Set<number>>, k: number, g: number) => m.set(k, (m.get(k) ?? new Set()).add(g));
        for (const { id, kit } of all)
          for (const d of kit.drafts) {
            if (d.went_here) add(next.drafts, d.id, id);
            for (const p of d.photos) if (p.went_here) add(next.photos, p.id, id);
          }
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
    <button key={label} type="button" aria-pressed={on} onClick={pick} className={cn("rounded px-2 py-0.5 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring", on ? "bg-ink text-background" : place ? "bg-accent-warn/15 text-accent-warn" : "bg-hover text-ink-soft hover:text-ink")}>
      {label}
    </button>
  );
  const editing = open === "new" ? null : pool.drafts.find((d) => d.id === open);
  return (
    <div className="flex min-h-96 flex-col md:flex-row">
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
                <span className="flex shrink-0 gap-1">
                  {d.photos.length === 0 ? <span className="text-xs text-ink-soft">{t(kitKeys.pool.noPhotos, "no photos")}</span> : d.photos.map((p) => <Thumb key={p} trade={cat.trade} country={cat.country} photo={p} className="size-9" />)}
                </span>
                <span className="flex w-20 shrink-0 flex-col items-end gap-1">
                  <span className="font-mono text-sm text-ink tabular-nums">{t(kitKeys.uses, "{n} uses", { n: d.uses })}</span>
                  <Dots went={went.drafts.get(d.id)?.size ?? 0} of={of} />
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
      <aside className="border-t border-border p-5 md:w-88 md:shrink-0 md:border-t-0 md:border-l">
        {open === null ? (
          <p className="text-sm text-ink-soft">{t(kitKeys.pool.pick, "Pick a draft to edit it, or add one.")}</p>
        ) : (
          <DraftEditor key={open} cat={cat} draft={editing ?? null} pool={pool} keywords={keywords} places={places} onDone={() => setOpen(null)} />
        )}
      </aside>
    </div>
  );
}

/** A draft's text (while never copied: a use is of the text it had), its places and its photos, in the order a reviewer saves them. */
function DraftEditor({ cat, draft, pool, keywords, places, onDone }: { cat: Cat; draft: KitDraft | null; pool: Pool; keywords: readonly string[]; places: readonly Area[]; onDone: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const [text, setText] = useState(draft?.text ?? "");
  const [photos, setPhotos] = useState(draft?.photos ?? []);
  const [tagged, setTagged] = useState(draft?.places ?? []);
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const copied = draft !== null && draft.uses > 0;
  const missing = keywords.filter((k) => !(draft?.keywords ?? []).includes(k));
  const path = { id: cat.trade, country: cat.country };
  const save = async () => {
    setBusy(true);
    try {
      if (draft === null) await ok(api.POST("/trades/{id}/{country}/kit/drafts", { params: { path }, body: { text: text.trim(), photos, places: tagged } }));
      else await ok(api.PATCH("/trades/{id}/{country}/kit/drafts/{draft}", { params: { path: { ...path, draft: draft.id } }, body: { ...(text.trim() !== draft.text && { text: text.trim() }), photos, places: tagged } }));
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
        {draft !== null && <Remove cat={cat} kind="drafts" id={draft.id} title={t(kitKeys.drafts.remove, "Remove the draft")} onDone={onDone} />}
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
      {pool.photos.length > 0 && (
        <div className="flex flex-col gap-1">
          <span className="text-sm text-ink">{t(kitKeys.pool.itsPhotos, "Its photos · order = save order")}</span>
          <span className="flex flex-wrap gap-1.5">
            {pool.photos.map((p) => {
              const n = photos.indexOf(p.id);
              return (
                <button key={p.id} type="button" aria-pressed={n >= 0} onClick={() => setPhotos(n >= 0 ? photos.filter((x) => x !== p.id) : [...photos, p.id])} className={cn("relative rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring", n >= 0 ? "ring-2 ring-ink" : "opacity-50 hover:opacity-80")} title={p.shows ?? undefined}>
                  <Thumb trade={cat.trade} country={cat.country} photo={p.id} className="size-11" />
                  {n >= 0 && <span className="absolute top-0.5 right-0.5 flex size-4 items-center justify-center rounded-full bg-ink text-xs text-background">{n + 1}</span>}
                </button>
              );
            })}
          </span>
        </div>
      )}
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

function Photos({ pool, cat, places, went, members }: { pool: Pool; cat: Cat; places: readonly Area[]; went: Went; members: readonly KitMember[] }) {
  const t = useT();
  const [open, setOpen] = useState<number | null>(null);
  const photo = pool.photos.find((p) => p.id === open);
  const silent = pool.photos.filter((p) => p.shows === null).length;
  return (
    <div className="flex min-h-96 flex-col md:flex-row">
      <section className="flex min-w-0 flex-1 flex-col gap-2 p-5">
        <h3 className={SECTION_HEAD}>
          {t(kitKeys.pool.leastUsed, "Least used first")}
          <span className="flex-1" />
          {silent > 0 && <span className={cn(HINT, "text-accent-warn")}>{t(kitKeys.pool.silent, "{n} say nothing", { n: silent })}</span>}
        </h3>
        <ul className="flex flex-wrap gap-3">
          {leastUsed(pool.photos).map((p) => (
            <li key={p.id}>
              <button type="button" onClick={() => setOpen(p.id)} className={cn("flex w-28 flex-col gap-1 rounded-lg border p-1.5 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring", open === p.id ? "border-ink" : "border-transparent")}>
                <span className="relative">
                  <Thumb trade={cat.trade} country={cat.country} photo={p.id} className="size-24" />
                  <span className="absolute bottom-1 left-1 rounded bg-black/70 px-1 font-mono text-xs text-white tabular-nums">{p.uses}×</span>
                </span>
                <span className="truncate text-xs text-ink">{p.shows ?? <span className="text-ink-soft">{t(kitKeys.pool.saysNothing, "says nothing")}</span>}</span>
                {p.places.map((s) => (
                  <Chip key={s} tone="place">
                    {places.find((a) => a.slug === s)?.name ?? s}
                  </Chip>
                ))}
                <Dots went={went.photos.get(p.id)?.size ?? 0} of={members.length} />
                {p.drafts.length === 0 && <span className="text-xs text-accent-warn">{t(kitKeys.pool.lone, "in no draft")}</span>}
              </button>
            </li>
          ))}
          <Upload cat={cat} />
        </ul>
        <p className="text-xs text-ink-soft">{t(kitKeys.pool.reencoded, "Uploads are re-encoded: JPEG ≤ 2048 px, upright, no EXIF — nothing of where it was taken leaves with it.")}</p>
      </section>
      <aside className="border-t border-border p-5 md:w-88 md:shrink-0 md:border-t-0 md:border-l">
        {photo === undefined ? <p className="text-sm text-ink-soft">{t(kitKeys.pool.pickPhoto, "Pick a photo: what it shows, where it goes.")}</p> : <PhotoEditor key={photo.id} cat={cat} photo={photo} pool={pool} places={places} savedOn={members.filter((m) => went.photos.get(photo.id)?.has(m.id))} onDone={() => setOpen(null)} />}
      </aside>
    </div>
  );
}

function PhotoEditor({ cat, photo, pool, places, savedOn, onDone }: { cat: Cat; photo: KitPhoto; pool: Pool; places: readonly Area[]; savedOn: readonly KitMember[]; onDone: () => void }) {
  const t = useT();
  const [shows, setShows] = useState(photo.shows ?? "");
  const path = { id: cat.trade, country: cat.country, file: photo.id };
  const patch = (body: { shows?: string; places?: string[] }) =>
    ok(api.PATCH("/trades/{id}/{country}/kit/photos/{file}", { params: { path }, body })).then(cat.reload, (e: unknown) => notifyFailure(e, t));
  const rides = pool.drafts.filter((d) => photo.drafts.includes(d.id));
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-baseline gap-2">
        <h3 className={cn(SECTION_HEAD, "flex-1")}>{t(kitKeys.pool.photo, "Photo · {n}×", { n: photo.uses })}</h3>
        <Remove cat={cat} kind="photos" id={photo.id} title={t(kitKeys.photos.remove, "Remove the photo")} onDone={onDone} />
      </div>
      <Thumb trade={cat.trade} country={cat.country} photo={photo.id} className="aspect-square size-auto w-full" />
      <label className="flex flex-col gap-1">
        <span className="text-sm text-ink">{t(kitKeys.pool.shows, "Shows")}</span>
        <Input value={shows} maxLength={60} onChange={(e) => setShows(e.target.value)} onBlur={() => shows.trim() !== (photo.shows ?? "") && void patch({ shows: shows.trim() })} onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()} />
        <span className="text-xs text-ink-soft">{t(kitKeys.pool.showsHint, "the reviewer sees it under the draft; it names nothing of the GBP")}</span>
      </label>
      <div className="flex flex-col gap-1">
        <span className="text-sm text-ink">{t(kitKeys.pool.only, "Only for GBPs in")}</span>
        <span className="flex flex-wrap gap-1">
          {places.map((a) => {
            const on = photo.places.includes(a.slug);
            return (
              <button key={a.slug} type="button" aria-pressed={on} onClick={() => void patch({ places: on ? photo.places.filter((x) => x !== a.slug) : [...photo.places, a.slug] })} className={cn("flex items-center gap-1 rounded px-2 py-0.5 text-xs outline-none", on ? "bg-accent-warn/15 text-accent-warn ring-1 ring-accent-warn" : "bg-hover text-ink-soft hover:text-ink")}>
                <MapPin aria-hidden className="size-3" />
                {a.name}
              </button>
            );
          })}
        </span>
        {photo.places.length === 0 && <span className="text-xs text-ink-soft">{t(kitKeys.pool.everywhere, "none: any GBP of the category")}</span>}
      </div>
      <div className="flex flex-col gap-1">
        <span className="text-sm text-ink">{t(kitKeys.pool.rides, "Rides with")}</span>
        {rides.length === 0 ? (
          <span className="text-xs text-accent-warn">{t(kitKeys.pool.lone.why, "no draft: never offered")}</span>
        ) : (
          rides.map((d) => (
            <span key={d.id} className="truncate text-xs text-ink-soft">
              #{d.id} {d.text} ({d.photos.indexOf(photo.id) + 1})
            </span>
          ))
        )}
      </div>
      {savedOn.length > 0 && (
        <div className="flex flex-col gap-1">
          <span className="text-sm text-ink">{t(kitKeys.pool.savedOn, "Saved on")}</span>
          <span className="text-xs text-ink-soft">{savedOn.map((m) => m.name).join(", ")} — {t(kitKeys.pool.never, "never offered there again")}</span>
        </div>
      )}
    </div>
  );
}

/** Out of every offer, its uses kept: a removed draft still keeps the GBPs it went to. */
function Remove({ cat, kind, id, title, onDone }: { cat: Cat; kind: "drafts" | "photos"; id: number; title: string; onDone: () => void }) {
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
        const path = { id: cat.trade, country: cat.country };
        if (kind === "drafts") await ok(api.DELETE("/trades/{id}/{country}/kit/drafts/{draft}", { params: { path: { ...path, draft: id } } }));
        else await ok(api.DELETE("/trades/{id}/{country}/kit/photos/{file}", { params: { path: { ...path, file: id } } }));
        cat.reload();
        onDone();
      }}
    />
  );
}

/** One request per photo, in turn; one past the server's size is refused here, the rest still go. */
function Upload({ cat }: { cat: Cat }) {
  const t = useT();
  const [sent, setSent] = useState<number | null>(null);
  const upload = async (files: File[]) => {
    for (const [i, file] of files.entries()) {
      const max = photoMax(file.type);
      if (max !== undefined && file.size > max) {
        toast.error(t(kitKeys.photos.tooLarge, "{name}: a photo is {mib} MiB at most", { name: file.name, mib: max / 2 ** 20 }));
        continue;
      }
      try {
        await uploadPhoto(cat.trade, cat.country, file, (f) => setSent((i + f) / files.length));
      } catch (e) {
        notifyFailure(e, t);
      }
    }
    setSent(null);
    cat.reload();
  };
  return (
    <li>
      <label className={cn("flex size-28 cursor-pointer flex-col items-center justify-center gap-1 rounded-md border border-dashed border-border text-xs text-ink-soft focus-within:ring-2 focus-within:ring-ring hover:bg-hover hover:text-ink", sent !== null && "pointer-events-none")}>
        <ImagePlus aria-hidden className="size-4" />
        {sent === null ? t(kitKeys.photos.upload, "Upload") : `${Math.round(sent * 100)}%`}
        <input
          type="file"
          accept="image/*"
          multiple
          className="sr-only"
          onChange={(e) => {
            const files = [...(e.target.files ?? [])];
            e.target.value = "";
            if (files.length > 0) void upload(files);
          }}
        />
      </label>
    </li>
  );
}
