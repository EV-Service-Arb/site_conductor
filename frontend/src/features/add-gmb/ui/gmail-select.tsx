"use client";

import { Button, Input, NativeSelect, NativeSelectOption } from "@evinvest/uikit";
import { useState } from "react";

import { addGmail, gmbs, move, useGmails } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { add as addKeys } from "../model/i18n";

const NEW = "new";

/** The caller's gmails, then "New gmail…", which adds one and picks it. */
export function GmailSelect({ value, onPick }: { value: number | null; onPick: (gmail: number) => void }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const gmails = useGmails();
  const [naming, setNaming] = useState(false);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);

  const add = async () => {
    setBusy(true);
    try {
      const made = await addGmail(typed.trim());
      gmails.reload();
      onPick(made.id);
      setNaming(false);
      setTyped("");
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  if (naming)
    return (
      <span className="flex items-center gap-2">
        <Input
          size={size}
          type="email"
          autoFocus
          autoCapitalize="none"
          value={typed}
          placeholder={t(addKeys.gmail.new.placeholder, "name@gmail.com")}
          aria-label={t(addKeys.gmail.new, "New gmail…")}
          onChange={(e) => setTyped(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              if (typed.trim() !== "") void add();
            }
            if (e.key === "Escape") {
              e.stopPropagation();
              setNaming(false);
            }
          }}
          className="min-w-0 flex-1"
        />
        <Button type="button" size={button("sm")} disabled={busy || typed.trim() === ""} onClick={() => void add()}>
          {t(addKeys.gmail.add, "Add")}
        </Button>
        <Button type="button" variant="ghost" size={button("sm")} onClick={() => setNaming(false)}>
          {t(move.cancel, "Cancel")}
        </Button>
      </span>
    );

  return (
    <NativeSelect
      size={size}
      wrapperClassName="w-full"
      aria-label={t(gmbs.gbp.gmail, "Gmail")}
      disabled={gmails.status !== "ok"}
      value={value === null ? "" : String(value)}
      onChange={(e) => {
        const v = e.target.value;
        if (v === NEW) setNaming(true);
        else if (v !== "") onPick(Number(v));
      }}
    >
      {value === null && <NativeSelectOption value="">{t(addKeys.gmail.pick, "Pick a gmail")}</NativeSelectOption>}
      {gmails.status === "ok" &&
        gmails.data.gmails.map((g) => (
          <NativeSelectOption key={g.id} value={String(g.id)}>
            {g.gmail}
          </NativeSelectOption>
        ))}
      <NativeSelectOption value={NEW}>{t(addKeys.gmail.new, "New gmail…")}</NativeSelectOption>
    </NativeSelect>
  );
}
