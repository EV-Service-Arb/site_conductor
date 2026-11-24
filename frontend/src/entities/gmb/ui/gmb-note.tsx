"use client";

import { useResource } from "@evinvest/api/react";
import { Textarea, cn } from "@evinvest/uikit";
import { useState } from "react";

import { api, ok } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

import { gmbs as gmbsKeys } from "../model/i18n";

export const NOTE_MAX = 4000;

/** The reader's own note on a GBP, written where it is read: saved as it is left. */
export function GmbNote({ gmb, className }: { gmb: number; className?: string }) {
  const t = useT();
  const note = useResource(api, "/gmbs/{id}/note", { params: { path: { id: gmb } } });
  // `null` while not written in: what is kept shows
  const [typed, setTyped] = useState<string | null>(null);
  if (note.status !== "ok") return null;
  const kept = note.data.note;
  const save = async () => {
    if (typed === null) return;
    const next = typed.trim() === "" ? null : typed.trim();
    try {
      if (next !== kept) await ok(api.PUT("/gmbs/{id}/note", { params: { path: { id: gmb } }, body: { note: next } }));
      note.reload();
      setTyped(null);
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <Textarea
      rows={1}
      size="sm"
      maxLength={NOTE_MAX}
      value={typed ?? kept ?? ""}
      placeholder={t(gmbsKeys.note.add, "Click to add a note")}
      aria-label={t(gmbsKeys.note, "Your note: only you see it")}
      title={t(gmbsKeys.note, "Your note: only you see it")}
      onFocus={() => setTyped((now) => now ?? kept ?? "")}
      onChange={(e) => setTyped(e.target.value)}
      onBlur={() => void save()}
      onKeyDown={(e) => {
        if (e.key !== "Escape") return;
        // leaves the note, not the window it is in
        e.stopPropagation();
        e.currentTarget.blur();
      }}
      className={cn("field-sizing-content max-h-40 min-h-0 resize-none border-transparent bg-transparent px-1 py-0.5 text-sm shadow-none hover:bg-hover/50 focus-visible:border-border focus-visible:bg-hover/50", className)}
    />
  );
}
