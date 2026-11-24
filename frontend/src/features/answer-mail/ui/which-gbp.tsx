"use client";

import { NativeSelect, NativeSelectOption, toast, useUndo } from "@evinvest/uikit";
import { useState } from "react";

import { useNow } from "@/entities/alerts";
import { STATE_NAME, under, useGmails } from "@/entities/gmb";
import { editCase, mail, settledText, standing, subjectOf } from "@/entities/mail";
import type { Gmb, MailCase } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useControlSize } from "@/shared/ui/touch";

/** Picks which of its gmail's GBPs a case is about, where its mail names none; `onChanged`: of a pick, its undo and its redo. */
export function WhichGbp({ c, onChanged, className }: { c: MailCase; onChanged: () => void; className: string }) {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  const control = useControlSize();
  const gmails = useGmails();
  const push = useUndo();
  const [busy, setBusy] = useState(false);
  if (gmails.status !== "ok") return null;
  const gbps = under(gmails.data, c.gmail_id);
  if (gbps.length === 0) return null;
  const set = (gmb: number | null) => editCase(c.id, { gmb }).finally(onChanged);
  const pick = async (gbp: Gmb) => {
    setBusy(true);
    try {
      const { case: named } = await set(gbp.id);
      toast(t(mail.which.named, "{subject}: {gbp}", { subject: subjectOf(c, t), gbp: gbp.name }), { description: named.state.is === "done" ? settledText(named.state.by, t, locale, now) : standing(named, t, locale, now).text });
      push({ label: t(mail.which.label, "{subject} given its GBP", { subject: subjectOf(c, t) }), undo: async () => void (await set(null)), redo: async () => void (await set(gbp.id)) });
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  return (
    <NativeSelect size={control === "lg" ? "lg" : "sm"} wrapperClassName={className} aria-label={t(mail.which, "Which GBP?")} disabled={busy} value="" onChange={(e) => void pick(gbps.find((g) => g.id === Number(e.target.value))!)}>
      <NativeSelectOption value="" disabled>
        {t(mail.which, "Which GBP?")}
      </NativeSelectOption>
      {gbps.map((g) => (
        <NativeSelectOption key={g.id} value={g.id}>
          {`${g.name} · ${STATE_NAME[g.state](t)}`}
        </NativeSelectOption>
      ))}
    </NativeSelect>
  );
}
