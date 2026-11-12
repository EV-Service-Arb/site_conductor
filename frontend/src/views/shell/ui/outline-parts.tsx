"use client";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Button,
  buttonVariants,
} from "@evinvest/uikit";
import { Trash2, X } from "lucide-react";
import { useState } from "react";

import { select, useTrash } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

export const ROW = "group flex min-w-0 items-center gap-1 rounded-md pr-1 text-sm hover:bg-hover/50";
/** Shown on the row's hover or focus only, so the names keep the width. */
export const ON_HOVER = "hidden group-hover:flex group-focus-within:flex";
/** How long a removal waits in the trash, for a remove dialog's `{kept}`; unsaid until the trash is read. */
export function useKept(): string {
  const t = useT();
  const trash = useTrash();
  if (trash.status !== "ok") return "";
  const { never_purge, keep_days } = trash.data.settings;
  return never_purge ? t("trash.for.ever") : t("trash.for.days", { days: keep_days });
}

export const HEADING = "flex items-center gap-1 px-2 pt-2 pb-1 text-xs font-medium tracking-wide text-ink-soft uppercase";

export function Remove({ title, body, remove, onRemoved }: { title: string; body: string; remove: () => Promise<void>; onRemoved: () => void }) {
  const button = useButtonSize();
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="ghost" size={button("sm")} icon aria-label={title} title={title} onClick={() => setOpen(true)}>
        <Trash2 aria-hidden />
      </Button>
      {open && <RemoveDialog title={title} body={body} remove={remove} onRemoved={onRemoved} onClose={() => setOpen(false)} />}
    </>
  );
}

/** The confirmation, open while mounted. */
export function RemoveDialog({ title, body, remove, onRemoved, onClose }: { title: string; body: string; remove: () => Promise<void>; onRemoved: () => void; onClose: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const act = async () => {
    try {
      await remove();
      onRemoved();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <AlertDialog open onOpenChange={(o) => !o && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{body}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel className={buttonVariants({ variant: "outline", size: button() })}>{t("move.cancel")}</AlertDialogCancel>
          <AlertDialogAction className={buttonVariants({ variant: "destructive", size: button() })} onClick={() => void act()}>
            {t("gmbs.remove")}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function ResetSelection() {
  const t = useT();
  return (
    <button type="button" onClick={() => select(null)} aria-label={t("gmbs.reset")} title={t("gmbs.reset")} className="rounded p-0.5 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
      <X aria-hidden className="size-3.5" />
    </button>
  );
}
