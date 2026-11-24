"use client";

import { NativeSelect, NativeSelectOption, useUndo } from "@evinvest/uikit";
import { useState } from "react";

import { under, useGmails } from "@/entities/gmb";
import { editCase, mail, subjectOf } from "@/entities/mail";
import type { MailCase } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

/** Picks which of its gmail's GBPs a case is about, where its mail names none. */
export function WhichGbp({ c, onChanged, className }: { c: MailCase; onChanged: () => void; className: string }) {
  const t = useT();
  const gmails = useGmails();
  const push = useUndo();
  const [busy, setBusy] = useState(false);
  if (gmails.status !== "ok") return null;
  const gbps = under(gmails.data, c.gmail_id);
  if (gbps.length === 0) return null;
  const set = (gmb: number | null) => editCase(c.id, { gmb }).finally(onChanged);
  const pick = async (gmb: number) => {
    setBusy(true);
    try {
      await set(gmb);
      push({ label: t(mail.which.label, "{subject} given its GBP", { subject: subjectOf(c, t) }), undo: async () => void (await set(null)), redo: async () => void (await set(gmb)) });
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  return (
    <NativeSelect size="sm" wrapperClassName={className} aria-label={t(mail.which, "Which GBP?")} disabled={busy} value="" onChange={(e) => void pick(Number(e.target.value))}>
      <NativeSelectOption value="" disabled>
        {t(mail.which, "Which GBP?")}
      </NativeSelectOption>
      {gbps.map((g) => (
        <NativeSelectOption key={g.id} value={g.id}>
          {g.name}
        </NativeSelectOption>
      ))}
    </NativeSelect>
  );
}
