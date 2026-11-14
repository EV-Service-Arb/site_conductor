"use client";

import { Button, ConfirmDialog, Skeleton, Textarea, cn, toast } from "@evinvest/uikit";
import { Check, ImagePlus, Pencil, Plus, Trash2 } from "lucide-react";
import { useState } from "react";

import { type Country, type KitDraft, type KitPhoto, type KitPool, PHOTO_MAX, addDraft, editDraft, kit, move, photoSrc, removeKitItem, uploadPhoto } from "@/entities/gmb";
import { ApiError } from "@/shared/api";
import { useT } from "@/shared/i18n";
import type { Resource } from "@/shared/lib/use-resource";
import { ErrorState } from "@/shared/ui/error-state";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

const SECTION = "flex flex-col gap-2";
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";
const HINT = "font-normal tracking-normal normal-case";

const leastUsed = <X extends { uses: number }>(xs: readonly X[]) => [...xs].sort((a, b) => a.uses - b.uses);

/** A category's review kit (13h): the drafts a reviewer copies, and the photos they save, each to a GBP once. */
export function KitPoolPane({ trade, country, pool }: { trade: number; country: Country; pool: Resource<KitPool> & { reload: () => void } }) {
  const t = useT();
  if (pool.status === "loading") return <Skeleton className="h-64 w-full" />;
  if (pool.status === "error") return <ErrorState failure={pool.failure} onRetry={pool.reload} />;
  const { drafts, photos } = pool.data;
  const at = { trade, country, photos, reload: pool.reload };
  return (
    <div className="flex flex-col gap-6">
      <section className={SECTION}>
        <h3 className={SECTION_HEAD}>
          {t(kit.drafts, "Drafts")} <span className="font-normal tabular-nums">{drafts.length}</span>
          <span className="flex-1" />
          <span className={HINT}>{t(kit.drafts.order, "least used first · a GBP gets each at most once")}</span>
        </h3>
        {drafts.length === 0 && <p className="text-sm text-ink-soft">{t(kit.drafts.none, "No drafts yet: a reviewer's page offers Google's form alone.")}</p>}
        <ul className="flex flex-col divide-y divide-border border-y border-border">
          {leastUsed(drafts).map((d) => (
            <DraftRow key={d.id} draft={d} at={at} />
          ))}
        </ul>
        <NewDraft at={at} />
      </section>
      <section className={SECTION}>
        <h3 className={SECTION_HEAD}>
          {t(kit.photos, "Photos")} <span className="font-normal tabular-nums">{photos.length}</span>
          <span className="flex-1" />
          <span className={HINT}>{t(kit.photos.order, "least used first · a GBP gets each at most once · a draft selects its own")}</span>
        </h3>
        <ul className="flex flex-wrap gap-2">
          <Upload at={at} />
          {leastUsed(photos).map((p) => (
            <PhotoTile key={p.id} photo={p} at={at} />
          ))}
        </ul>
      </section>
    </div>
  );
}

interface At {
  trade: number;
  country: Country;
  photos: readonly KitPhoto[];
  reload: () => void;
}

function Thumb({ at, photo, className }: { at: At; photo: number; className?: string }) {
  // eslint-disable-next-line @next/next/no-img-element -- the API's own JPEG, behind the session: nothing for next/image to optimise in an export
  return <img src={photoSrc(at.trade, at.country, photo)} alt="" loading="lazy" className={cn("size-12 rounded-md object-cover", className)} />;
}

function DraftRow({ draft, at }: { draft: KitDraft; at: At }) {
  const t = useT();
  const button = useButtonSize();
  const [editing, setEditing] = useState(false);
  if (editing)
    return (
      <li className="py-3">
        <DraftEditor
          at={at}
          initial={draft}
          save={(text, photos) => editDraft(at.trade, at.country, draft.id, { ...(text !== draft.text && { text }), photos })}
          onDone={() => setEditing(false)}
        />
      </li>
    );
  return (
    <li className="flex items-center gap-3 py-2.5">
      <p className="line-clamp-2 min-w-0 flex-1 text-sm text-ink">{draft.text}</p>
      <span className="flex shrink-0 gap-1">
        {draft.photos.map((p) => (
          <Thumb key={p} at={at} photo={p} className="size-9" />
        ))}
      </span>
      <span className="w-16 shrink-0 text-right font-mono text-sm text-ink tabular-nums">{t(kit.uses, "{n} uses", { n: draft.uses })}</span>
      <Button variant="ghost" size={button("sm")} icon aria-label={t(kit.drafts.edit, "Edit the draft")} title={t(kit.drafts.edit, "Edit the draft")} onClick={() => setEditing(true)}>
        <Pencil aria-hidden />
      </Button>
      <Remove at={at} kind="drafts" id={draft.id} title={t(kit.drafts.remove, "Remove the draft")} />
    </li>
  );
}

/** Out of every offer, its uses kept: a removed draft still keeps the GBPs it went to. */
function Remove({ at, kind, id, title }: { at: At; kind: "drafts" | "photos"; id: number; title: string }) {
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
      description={t(kit.remove.body, "Out of every offer from now on. The GBPs it went to keep it as used.")}
      confirmLabel={t(kit.remove, "Remove")}
      labels={{ cancel: t(move.cancel, "Cancel") }}
      onConfirm={async () => {
        await removeKitItem(at.trade, at.country, kind, id);
        at.reload();
      }}
    />
  );
}

function NewDraft({ at }: { at: At }) {
  const t = useT();
  const button = useButtonSize();
  const [open, setOpen] = useState(false);
  if (!open)
    return (
      <Button variant="outline" size={button("sm")} className="self-start" onClick={() => setOpen(true)}>
        <Plus aria-hidden />
        {t(kit.drafts.add, "Add a draft")}
      </Button>
    );
  return <DraftEditor at={at} initial={{ text: "", photos: [] }} save={(text, photos) => addDraft(at.trade, at.country, text, photos)} onDone={() => setOpen(false)} />;
}

/** A draft's text and the photos it carries; the server's refusal (a copied draft's text) shown under it. */
function DraftEditor({ at, initial, save, onDone }: { at: At; initial: { text: string; photos: number[] }; save: (text: string, photos: number[]) => Promise<unknown>; onDone: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const [text, setText] = useState(initial.text);
  const [photos, setPhotos] = useState(initial.photos);
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    try {
      await save(text.trim(), photos);
      at.reload();
      onDone();
    } catch (e) {
      if (!(e instanceof ApiError)) throw e;
      if (e.failure.kind !== "unauthenticated") setRefused(failureText(e.failure, t));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <Textarea autoFocus rows={3} value={text} onChange={(e) => setText(e.target.value)} placeholder={t(kit.drafts.placeholder, "A review as a customer would write it, in the country's language")} aria-label={t(kit.drafts.text, "The draft's text")} />
      {at.photos.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-xs text-ink-soft">{t(kit.drafts.photos, "Its photos")}</span>
          {at.photos.map((p) => {
            const on = photos.includes(p.id);
            return (
              <button
                key={p.id}
                type="button"
                aria-pressed={on}
                onClick={() => setPhotos(on ? photos.filter((x) => x !== p.id) : [...photos, p.id])}
                className={cn("relative rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring", on ? "ring-2 ring-ink" : "opacity-50 hover:opacity-80")}
              >
                <Thumb at={at} photo={p.id} className="size-10" />
                {on && <Check aria-hidden className="absolute top-0.5 right-0.5 size-3.5 rounded-full bg-ink p-0.5 text-background" />}
              </button>
            );
          })}
        </div>
      )}
      {refused !== null && <p className="text-xs text-accent-error">{refused}</p>}
      <span className="flex gap-2">
        <Button type="submit" size={button("sm")} disabled={busy || text.trim() === ""}>
          {t(kit.save, "Save")}
        </Button>
        <Button type="button" variant="ghost" size={button("sm")} onClick={onDone}>
          {t(move.cancel, "Cancel")}
        </Button>
      </span>
    </form>
  );
}

function PhotoTile({ photo, at }: { photo: KitPhoto; at: At }) {
  const t = useT();
  return (
    <li className="group relative">
      <Thumb at={at} photo={photo.id} className="size-20" />
      <span className="absolute bottom-1 left-1 rounded bg-black/70 px-1 font-mono text-xs text-white tabular-nums">{photo.uses}×</span>
      <span className="absolute top-0 right-0 hidden rounded-md bg-background/80 group-focus-within:block group-hover:block">
        <Remove at={at} kind="photos" id={photo.id} title={t(kit.photos.remove, "Remove the photo")} />
      </span>
    </li>
  );
}

/** One request per photo, in turn; one past the server's size is refused here, the rest still go. */
function Upload({ at }: { at: At }) {
  const t = useT();
  const [sent, setSent] = useState<number | null>(null);
  const upload = async (files: File[]) => {
    for (const [i, file] of files.entries()) {
      if (file.size > PHOTO_MAX) {
        toast.error(t(kit.photos.tooLarge, "{name}: a photo is 10 MiB at most", { name: file.name }));
        continue;
      }
      try {
        await uploadPhoto(at.trade, at.country, file, (f) => setSent((i + f) / files.length));
      } catch (e) {
        notifyFailure(e, t);
      }
    }
    setSent(null);
    at.reload();
  };
  return (
    <li>
      <label className={cn("flex size-20 cursor-pointer flex-col items-center justify-center gap-1 rounded-md border border-dashed border-border text-xs text-ink-soft hover:bg-hover hover:text-ink focus-within:ring-2 focus-within:ring-ring", sent !== null && "pointer-events-none")}>
        <ImagePlus aria-hidden className="size-4" />
        {sent === null ? t(kit.photos.upload, "Upload") : `${Math.round(sent * 100)}%`}
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
