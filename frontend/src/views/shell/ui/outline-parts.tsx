"use client";

import { Button } from "@evinvest/uikit";
import { Trash2, X } from "lucide-react";

import { select, useTrash, gmbs } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

export const ROW = "group flex min-w-0 items-center gap-1 rounded-md pr-1 text-sm hover:bg-hover/50";
/** Shown on the row's hover or focus only, so the names keep the width. */
export const ON_HOVER = "hidden group-hover:flex group-focus-within:flex";
export const HEADING = "flex items-center gap-1 px-2 pt-2 pb-1 text-xs font-medium tracking-wide text-ink-soft uppercase";

/** Removes to the trash, re-read after so its button flashes the new count. */
export function useRemove(onRemoved: () => void): (remove: () => Promise<void>) => Promise<void> {
  const t = useT();
  const trash = useTrash();
  return async (remove) => {
    try {
      await remove();
    } catch (e) {
      notifyFailure(e, t);
      return;
    }
    onRemoved();
    trash.reload();
  };
}

export function Remove({ title, remove, onRemoved }: { title: string; remove: () => Promise<void>; onRemoved: () => void }) {
  const button = useButtonSize();
  const run = useRemove(onRemoved);
  return (
    <Button variant="ghost" size={button("sm")} icon aria-label={title} title={title} onClick={() => void run(remove)}>
      <Trash2 aria-hidden />
    </Button>
  );
}

export function ResetSelection() {
  const t = useT();
  return (
    <button type="button" onClick={() => select(null)} aria-label={t(gmbs.reset, "Reset selection")} title={t(gmbs.reset, "Reset selection")} className="rounded p-0.5 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
      <X aria-hidden className="size-3.5" />
    </button>
  );
}
