"use client";

import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, buttonVariants } from "@evinvest/uikit";
import { type ReactNode, useState } from "react";

import type { Category, Listing, Release } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { adoptRelease } from "../api/categories";
import { categoryName } from "../model/category";
import { useCategories } from "../model/gmails-context";
import { move, trades as tradesKeys } from "../model/i18n";
import { VersionDiff, VersionRead } from "./version-diff";

type Pending = { listing: Listing; release: Release; held: Category; then: (adopted: Category) => void };

/** A listing's release made the caller's category's head: at once where they hold none, after its diff with theirs where they do. */
export function useAdopt(): { adopt: (listing: Listing, release: Release, then: (adopted: Category) => void) => void; dialog: ReactNode } {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const categories = useCategories();
  const [pending, setPending] = useState<Pending | null>(null);
  const run = async (listing: Listing, release: Release, then: (adopted: Category) => void) => {
    try {
      const adopted = await adoptRelease(listing.trade.id, listing.country, release);
      categories.reload();
      then(adopted);
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  const adopt = (listing: Listing, release: Release, then: (adopted: Category) => void) => {
    if (categories.status !== "ok") throw new Error("a listing adopted before the categories are read");
    const held = categories.data.find((c) => c.trade.id === listing.trade.id && c.country === listing.country);
    if (held === undefined) return void run(listing, release, then);
    if (held.head.id === release.version) return then(held);
    setPending({ listing, release, held, then });
  };
  const dialog = (
    <AlertDialog open={pending !== null} onOpenChange={(open) => !open && setPending(null)}>
      {pending !== null && (
        <AlertDialogContent className="max-w-2xl">
          <AlertDialogHeader>
            <AlertDialogTitle>{t(tradesKeys.adopt.title, "Use {listing} v{n}?", { listing: pending.listing.title, n: pending.release.n })}</AlertDialogTitle>
            <AlertDialogDescription>
              {t(tradesKeys.adopt.what, "Your {category} moves onto it, as below. Its history keeps where it was: Revert takes it back.", { category: categoryName(pending.held.trade, pending.held.country, locale) })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <VersionRead id={pending.release.version}>{(v) => <VersionDiff from={pending.held.head} to={v} />}</VersionRead>
          <AlertDialogFooter>
            <AlertDialogCancel className={buttonVariants({ variant: "outline", size: button() })}>{t(move.cancel, "Cancel")}</AlertDialogCancel>
            <AlertDialogAction
              className={buttonVariants({ size: button() })}
              onClick={() => {
                setPending(null);
                void run(pending.listing, pending.release, pending.then);
              }}
            >
              {t(tradesKeys.adopt.use, "Use it")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      )}
    </AlertDialog>
  );
  return { adopt, dialog };
}
