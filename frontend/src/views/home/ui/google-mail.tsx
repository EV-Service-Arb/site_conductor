"use client";

import { cn } from "@evinvest/uikit";
import { ChevronDown } from "lucide-react";
import { useState } from "react";

import { type Listed, gmbsOf, goes, needsOf, useGmails, useMailCases, windowKeys } from "@/entities/gmb";
import { Automation, Standing, TopicBadge, over, pressing, subjectOf, useAutos, useGist } from "@/entities/mail";
import { Conversation } from "@/features/answer-mail";
import { AutoOf } from "@/features/auto-mail";
import type { MailAutos, MailCase } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { ErrorState } from "@/shared/ui/error-state";

/** The GBP's open conversations with Google, whatever their topic, and whether they are answered by themselves; `unfolded`: what presses most starts open. */
export function GoogleMail({ listed: { gmb, gmail }, unfolded }: { listed: Listed; unfolded: boolean }) {
  const t = useT();
  const cases = useMailCases();
  const autos = useAutos();
  // one answered at a time; what presses is settled once they are read, so none moves from under its operator
  const [presses, setPresses] = useState<number | null>();
  const [picked, setPicked] = useState<number | null>();
  const opened = picked !== undefined ? picked : unfolded ? presses : null;
  if (cases.status === "error")
    return (
      <div className="px-5 pt-5">
        <ErrorState failure={cases.failure} onRetry={cases.reload} />
      </div>
    );
  if (cases.status === "loading") return null;
  const open = pressing(cases.data.cases.filter((c) => !over(c) && gmbsOf(c).includes(gmb.id)));
  const [first] = open;
  if (first === undefined) return null;
  if (presses === undefined) setPresses(needsOf(first) !== null || goes(first) ? first.id : null);
  return (
    <section className="flex flex-col border-b border-border px-5 pt-3 pb-2">
      <h3 className="text-sm font-semibold text-ink">
        {t(windowKeys.mail, "With Google")} <span className="font-normal text-ink-soft tabular-nums">{open.length}</span>
      </h3>
      <ul className="flex flex-col divide-y divide-border">
        {open.map((c) => (
          <Row key={c.id} c={c} gmb={gmb.id} autos={autos.status === "ok" ? autos.data : null} open={c.id === opened} onToggle={() => setPicked(c.id === opened ? null : c.id)} />
        ))}
      </ul>
      {gmail !== null && <AutoOf gmb={gmb} gmail={gmail} auto={autos} />}
    </section>
  );
}

function Row({ c, gmb, autos, open, onToggle }: { c: MailCase; gmb: number; autos: MailAutos | null; open: boolean; onToggle: () => void }) {
  const t = useT();
  const gist = useGist(c);
  const gmails = useGmails();
  const others = gmails.status === "ok" ? gmbsOf(c).filter((id) => id !== gmb).map((id) => gmails.data.gmbs.find((g) => g.id === id)?.name ?? t(windowKeys.mail.gone, "one in the trash")) : [];
  return (
    <li className="flex flex-col">
      <button type="button" aria-expanded={open} onClick={onToggle} className="flex min-w-0 items-start gap-2 rounded-md py-2 text-left outline-none hover:bg-hover/40 focus-visible:ring-2 focus-visible:ring-ring">
        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
            <TopicBadge topic={c.topic} />
            <span className="min-w-0 flex-1 basis-40 truncate text-sm text-ink">{(open ? null : gist) ?? subjectOf(c, t)}</span>
            <Standing c={c} />
          </span>
          {c.gmb_id === gmb && !c.gmb_certain && <span className="text-xs text-accent-warn">{t(windowKeys.mail.likely, "Likely about this one: not certain")}</span>}
          {others.length > 0 && <span className="text-xs text-ink">{t(windowKeys.mail.also, "Also about {names}", { names: others.join(", ") })}</span>}
          {autos !== null && <Automation c={c} autos={autos} />}
        </span>
        <ChevronDown aria-hidden className={cn("mt-0.5 size-4 shrink-0 text-ink-soft", open && "rotate-180")} />
      </button>
      {open && (
        <div className="pt-1 pb-4">
          <Conversation id={c.id} />
        </div>
      )}
    </li>
  );
}
