"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, Field, FieldLabel, Input, NativeSelect, NativeSelectOption, Textarea } from "@evinvest/uikit";
import { type ReactNode, useId, useState } from "react";

import { api, ok } from "@/shared/api";
import type { Category, CategoryVersion, Country, GoogleCategory, Lineage, Listing, StudyRelease } from "@/shared/api/schema";
import { state, useLocale, useT } from "@/shared/i18n";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { categoryName } from "../model/category";
import { useCategories } from "../model/gmails-context";
import { trades as tradesKeys } from "../model/i18n";
import { VersionDiff, VersionRead } from "./version-diff";

const MAX_TITLE = 100;

/** Category version `version` released as `release` says: a new listing, or the next version of one of the caller's. */
function publishRelease(version: number, release: StudyRelease): Promise<Listing> {
  if (release.as === "new") return ok(api.POST("/library", { body: { version, title: release.title, about: release.about, notes: release.notes, ...(release.forked_from != null && { forked_from: release.forked_from }) } }));
  return ok(api.POST("/library/{id}/versions", { params: { path: { id: release.listing } }, body: { version, notes: release.notes } }));
}

/**
 * How `body` is to be released: as a new listing (forked off `fork`, else off the listing its head came from when that is
 * not the caller's), or the next version of one of the caller's listings of its trade and country, with what changed since
 * that one's last version. `release` is `null` while it is not one yet.
 */
export function useRelease({ trade, country, body, lineage, fork, title: initial }: { trade: GoogleCategory; country: Country; body: CategoryVersion; lineage: Lineage | null; fork: Listing | null; title: string }): { fields: ReactNode; release: StudyRelease | null } {
  const t = useT();
  const id = useId();
  const size = useControlSize();
  const mine = useResource(api, "/library", { params: { query: { kind: "mine", sort: "recent", trade: trade.id, country } } });
  const [picked, setPicked] = useState<"new" | number | null>(null);
  const [title, setTitle] = useState(initial);
  const [about, setAbout] = useState("");
  const [notes, setNotes] = useState("");
  const own = mine.status === "ok" ? mine.data.listings.filter((l) => l.withdrawn_at === null) : [];
  const lineageMine = lineage !== null && own.some((l) => l.id === lineage.listing);
  const choice = picked ?? (fork === null && lineageMine ? lineage!.listing : "new");
  const next = choice === "new" ? null : own.find((l) => l.id === choice)!;
  const forked = fork?.id ?? (lineage !== null && !lineageMine ? lineage.listing : null);
  const forkedTitle = fork?.title ?? (lineage !== null && !lineageMine ? lineage.title : null);
  const named = title.trim() !== "" && title.trim().length <= MAX_TITLE;
  const release: StudyRelease | null =
    mine.status !== "ok" ? null : next !== null ? { as: "next", listing: next.id, notes: notes.trim() } : named ? { as: "new", title: title.trim(), about: about.trim(), notes: notes.trim(), ...(forked !== null && { forked_from: forked }) } : null;
  const fields = (
    <div className="flex flex-col gap-3">
      {mine.status === "loading" && <p className="text-sm text-ink-soft">{t(state.loading, "Loading…")}</p>}
      {mine.status === "error" && <p className="text-sm text-accent-error">{failureText(mine.failure, t)}</p>}
      {own.length > 0 && (
        <NativeSelect size="sm" aria-label={t(tradesKeys.release.as, "Release as")} value={String(choice)} onChange={(e) => setPicked(e.currentTarget.value === "new" ? "new" : Number(e.currentTarget.value))}>
          <NativeSelectOption value="new">{t(tradesKeys.release.new, "A new listing")}</NativeSelectOption>
          {own.map((l) => (
            <NativeSelectOption key={l.id} value={String(l.id)}>
              {t(tradesKeys.release.next, "{listing} v{n}, the next version", { listing: l.title, n: l.latest.n + 1 })}
            </NativeSelectOption>
          ))}
        </NativeSelect>
      )}
      {next === null ? (
        <>
          <Field className="flex flex-col gap-1">
            <FieldLabel htmlFor={`${id}-title`}>{t(tradesKeys.release.title, "Title")}</FieldLabel>
            <Input id={`${id}-title`} size={size} maxLength={MAX_TITLE} value={title} onChange={(e) => setTitle(e.target.value)} />
          </Field>
          <Field className="flex flex-col gap-1">
            <FieldLabel htmlFor={`${id}-about`}>{t(tradesKeys.release.about, "About")}</FieldLabel>
            <Textarea id={`${id}-about`} size={size} rows={2} value={about} onChange={(e) => setAbout(e.target.value)} />
          </Field>
          {forkedTitle !== null && <p className="text-xs text-ink-soft">{t(tradesKeys.release.forked, "Forked from {listing}", { listing: forkedTitle })}</p>}
        </>
      ) : (
        <section className="flex flex-col gap-1">
          <span className="text-xs text-ink-soft">{t(tradesKeys.release.since, "Since {listing} v{n}", { listing: next.title, n: next.latest.n })}</span>
          <VersionRead id={next.latest.version}>{(v) => <VersionDiff from={v} to={body} />}</VersionRead>
        </section>
      )}
      <Field className="flex flex-col gap-1">
        <FieldLabel htmlFor={`${id}-notes`}>{t(tradesKeys.release.notes, "Notes")}</FieldLabel>
        <Textarea id={`${id}-notes`} size={size} rows={2} value={notes} onChange={(e) => setNotes(e.target.value)} placeholder={t(tradesKeys.release.notes.what, "What this version changes, and why")} />
      </Field>
      <p className="rounded-md border border-accent-warn/40 bg-accent-warn/5 px-3 py-2 text-xs text-ink">{t(tradesKeys.release.public, "A release is readable by everyone, for good: withdrawing it later hides it from new readers only.")}</p>
    </div>
  );
  return { fields, release };
}

/** The category's head released: a new listing (off `fork` where given) or the next version of one of the caller's. */
export function PublishDialog({ category, fork, onClose }: { category: Category; fork: Listing | null; onClose: () => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const categories = useCategories();
  const [busy, setBusy] = useState(false);
  const { fields, release } = useRelease({ trade: category.trade, country: category.country, body: category.head, lineage: category.lineage, fork, title: fork?.title ?? categoryName(category.trade, category.country, locale) });
  const submit = async () => {
    if (release === null) throw new Error("published before the release was whole");
    setBusy(true);
    try {
      await publishRelease(category.head.id, release);
      categories.reload();
      onClose();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t(tradesKeys.release.dialog, "Publish {category}", { category: categoryName(category.trade, category.country, locale) })}</DialogTitle>
          <DialogDescription>{t(tradesKeys.release.dialog.what, "Its head, as it stands, into the library: anyone may use it, fork it, and be told of your next versions.")}</DialogDescription>
        </DialogHeader>
        {fields}
        <DialogFooter>
          <Button size={button()} disabled={busy || release === null} onClick={() => void submit()}>
            {t(tradesKeys.release.publish, "Publish")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
