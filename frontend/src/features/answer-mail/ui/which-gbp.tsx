"use client";

import { Button, NativeSelect, NativeSelectGroup, NativeSelectOption, toast, useUndo } from "@evinvest/uikit";
import { Check } from "lucide-react";
import { useState } from "react";

import { useNow } from "@/entities/alerts";
import { STATE_NAME, under, useGmails } from "@/entities/gmb";
import { editCase, mail, settledText, standing, subjectOf } from "@/entities/mail";
import type { Gmb, MailCase } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

/** A case given its GBP by hand; `onChanged`: of that, its undo and its redo. */
function usePick(c: MailCase, onChanged: () => void) {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  const push = useUndo();
  const [busy, setBusy] = useState(false);
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
  return { busy, pick };
}

/** The GBPs of the owner's under their gmails, `gmail`'s first. */
export function GbpOptions({ gmail }: { gmail: number }) {
  const t = useT();
  const gmails = useGmails();
  if (gmails.status !== "ok") return null;
  const { data } = gmails;
  return [...data.gmails]
    .sort((a, b) => Number(b.id === gmail) - Number(a.id === gmail))
    .map((g) => ({ g, gbps: under(data, g.id) }))
    .filter(({ gbps }) => gbps.length > 0)
    .map(({ g, gbps }) => (
      <NativeSelectGroup key={g.id} label={g.gmail}>
        {gbps.map((b) => (
          <NativeSelectOption key={b.id} value={b.id}>
            {`${b.name} · ${STATE_NAME[b.state](t)}`}
          </NativeSelectOption>
        ))}
      </NativeSelectGroup>
    ));
}

/** Picks which of its owner's GBPs a case is about, where its mail names none. */
export function WhichGbp({ c, onChanged, className }: { c: MailCase; onChanged: () => void; className: string }) {
  const t = useT();
  const control = useControlSize();
  const gmails = useGmails();
  const { busy, pick } = usePick(c, onChanged);
  if (gmails.status !== "ok" || gmails.data.gmbs.length === 0) return null;
  const { gmbs } = gmails.data;
  return (
    <NativeSelect size={control === "lg" ? "lg" : "sm"} wrapperClassName={className} aria-label={t(mail.which, "Which GBP?")} disabled={busy} value="" onChange={(e) => void pick(gmbs.find((g) => g.id === Number(e.target.value))!)}>
      <NativeSelectOption value="" disabled>
        {t(mail.which, "Which GBP?")}
      </NativeSelectOption>
      <GbpOptions gmail={c.gmail_id} />
    </NativeSelect>
  );
}

/** Confirms the GBP a case is likely about. */
export function Likely({ c, onChanged }: { c: MailCase; onChanged: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const gmails = useGmails();
  const { busy, pick } = usePick(c, onChanged);
  const gbp = gmails.status === "ok" ? gmails.data.gmbs.find((g) => g.id === c.gmb_id) : undefined;
  if (gbp === undefined || c.gmb_certain) return null;
  return (
    <Button variant="outline" size={button("xs")} disabled={busy} title={t(mail.likely.why, "Nothing of it goes by itself until its GBP is certain")} onClick={() => void pick(gbp)} className="h-auto min-h-8 max-w-full self-start py-1 text-left whitespace-normal">
      <Check aria-hidden />
      <span className="min-w-0 break-words">{t(mail.likely.confirm, "Likely {name}: confirm", { name: gbp.name })}</span>
    </Button>
  );
}
