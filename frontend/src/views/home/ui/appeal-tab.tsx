"use client";

import { Badge, Button, Planned } from "@evinvest/uikit";
import { ShieldAlert } from "lucide-react";

import { type Listed, STATE_NAME, STATE_TONE, openWindow, useGmails, useGmbTags, windowKeys } from "@/entities/gmb";
import type { Gmb } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";

const host = (site: string | null) => (site !== null && /^https?:\/\//.test(site) ? new URL(site).hostname.replace(/^www\./, "") : null);

/** A suspended GBP's appeal (05h). */
export function AppealTab({ listed: { gmb, gmail } }: { listed: Listed }) {
  const t = useT();
  const gmails = useGmails();
  const { tags } = useGmbTags();
  const mine = host(gmb.website);
  const brand = tags.sites?.get(gmb.id)?.brand;
  const others = gmails.status !== "ok" ? [] : gmails.data.gmbs.filter((g) => g.id !== gmb.id);
  const sameSite = others.filter((g) => (mine !== null && host(g.website) === mine) || (brand !== undefined && tags.sites?.get(g.id)?.brand === brand));
  const sameGmail = others.filter((g) => gmb.gmail_id !== null && g.gmail_id === gmb.gmail_id && !sameSite.includes(g));
  const open = (g: Gmb) => {
    const first = g.tabs[0];
    if (first === undefined) throw new Error(`GBP ${g.id} has no tabs`);
    openWindow({ kind: "gbp", gmb: g.id, tab: first.tab });
  };
  const group = (what: string, list: Gmb[]) =>
    list.length === 0 ? null : (
      <div className="flex flex-col gap-1">
        <p className="font-mono text-xs text-ink-soft">{what}</p>
        {list.map((g) => (
          <Button key={g.id} variant="ghost" size="sm" className="justify-start gap-2" onClick={() => open(g)}>
            <Badge tone={STATE_TONE[g.state]}>{STATE_NAME[g.state](t)}</Badge>
            <span className="min-w-0 flex-1 truncate text-left">{g.name}</span>
          </Button>
        ))}
      </div>
    );
  return (
    <div className="@container flex flex-col gap-4 p-5">
      <section className="flex items-center gap-4 rounded-lg border border-accent-error/40 bg-accent-error/5 px-4 py-3">
        <ShieldAlert aria-hidden className="size-5 shrink-0 text-accent-error" />
        <p className="min-w-0 flex-1 text-sm text-ink-soft">{t(windowKeys.appeal.banner, "Google wants every document to match the profile's exact name and address.")}</p>
        <Button variant="outline" planned={t(windowKeys.appeal.resubmit, "Replace a document and resubmit the appeal")}>
          {t(windowKeys.appeal.resubmit.label, "Resubmit")}
        </Button>
      </section>
      <div className="grid grid-cols-1 gap-4 @3xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <div className="flex flex-col gap-4">
          <Planned what={t(windowKeys.appeal.documents.what, "The documents of the appeal, each checked for the profile's exact name and address")}>
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.appeal.documents, "Documents")}</h3>
            <p className="rounded-md bg-hover px-3 py-2 font-mono text-xs text-ink">profile · {gmb.name}{gmb.address && ` · ${gmb.address}`}</p>
            {[
              ["Kbis extract", true],
              ["EDF bill, Oct", false],
              ["Signage + business card photo", true],
            ].map(([doc, named]) => (
              <p key={String(doc)} className="flex items-center gap-2 text-sm text-ink">
                <span className="flex-1">{doc}</span>
                <span className={named ? "text-positive" : "text-accent-error"}>name</span>
                <span className="text-positive">address</span>
              </p>
            ))}
          </Planned>
          <Planned what={t(windowKeys.appeal.sent.what, "The appeals sent for this profile, one per try, and what Google answered")}>
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.appeal.sent, "Sent")}</h3>
            <p className="flex justify-between font-mono text-xs text-ink"><span>Nov 7 · appeal 1 · by hand</span><span>no decision</span></p>
            <p className="flex justify-between font-mono text-xs text-ink"><span>Nov 12 · appeal 2 · by the rule</span><span>no decision</span></p>
          </Planned>
        </div>
        <div className="flex flex-col gap-4">
          <section className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3">
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.appeal.linked, "Linked to it")}</h3>
            <p className="text-xs text-ink-soft">{t(windowKeys.appeal.linked.hint, "The site or the gmail ties to this profile: a shared site is the risk, not the name.")}</p>
            {group(t(windowKeys.appeal.linked.site, "same site{host}", { host: mine === null ? "" : ` · ${mine}` }), sameSite)}
            {group(t(windowKeys.appeal.linked.gmail, "same gmail{gmail}", { gmail: gmail === null ? "" : ` · ${gmail.gmail}` }), sameGmail)}
            {sameSite.length === 0 && sameGmail.length === 0 && <p className="text-sm text-ink-soft">{t(windowKeys.appeal.linked.none, "No other GBP shares its site or its gmail.")}</p>}
          </section>
          <Planned what={t(windowKeys.appeal.rules.what, "Email rules on its gmail: re-submit the appeal, move its state on Google's decision mail")}>
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.appeal.rules, "Email rules")}</h3>
            <p className="text-sm text-ink">Re-submit appeal · open 5 d with no decision · max 4</p>
            <p className="text-sm text-ink">Appeal mail → state · reinstated → Verified</p>
          </Planned>
        </div>
      </div>
    </div>
  );
}
