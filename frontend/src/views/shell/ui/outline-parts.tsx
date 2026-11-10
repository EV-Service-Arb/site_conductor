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
import { Trash2 } from "lucide-react";
import { useState } from "react";

import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

export const ROW = "group flex min-w-0 items-center gap-1 rounded-md pr-1 text-sm hover:bg-hover/50";
/** Shown on the row's hover or focus only, so the names keep the width. */
export const ON_HOVER = "hidden group-hover:flex group-focus-within:flex";
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
