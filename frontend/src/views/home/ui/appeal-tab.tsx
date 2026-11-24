"use client";

import { Badge, Button, ConfirmDialog, Planned, Skeleton } from "@evinvest/uikit";
import { ShieldAlert, Trash2 } from "lucide-react";
import { useState } from "react";

import { AttachFiles, type Listed, STATE_NAME, STATE_TONE, keepDocument, fileSize, openWindow, removeDocument, saveDocument, useDocuments, useGmails, useGmbTags, windowKeys } from "@/entities/gmb";
import { MailLines, useGmbMail } from "@/entities/mail";
import type { Gmb, GmbDocument, MailKind, MailLine } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { ViewLink } from "@/shared/lib/location";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

const host = (site: string | null) => (site !== null && /^https?:\/\//.test(site) ? new URL(site).hostname.replace(/^www\./, "") : null);

/** What Google decided of its suspension. */
const DECISIONS: readonly (MailKind | null)[] = ["suspended", "account_restricted", "appeal_denied", "appeal_approved"];

/** A suspended GBP's appeal (05h). */
export function AppealTab({ listed: { gmb, gmail } }: { listed: Listed }) {
  const t = useT();
  const gmails = useGmails();
  const { tags } = useGmbTags();
  const mail = useGmbMail(gmb.id);
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
  const cases = mail.status !== "ok" ? [] : mail.data.cases.filter((c) => c.topic === "suspension");
  const told = mail.status !== "ok" ? [] : mail.data.lines.filter((l) => cases.some((c) => c.id === l.case_id));
  // newest first
  const decision = told.find((l) => DECISIONS.includes(l.kind));
  const rest = told.filter((l) => l !== decision);
  return (
    <div className="@container flex flex-col gap-4 p-5">
      <section className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-lg border border-accent-error/40 bg-accent-error/5 px-4 py-3">
        <ShieldAlert aria-hidden className="size-5 shrink-0 text-accent-error" />
        <div className="flex min-w-0 flex-1 basis-60 flex-col gap-1">
          {decision !== undefined ? <Decision l={decision} /> : mail.status === "ok" && <p className="text-sm text-ink">{t(windowKeys.verification.banner.none, "Nothing from Google yet")}</p>}
        </div>
        <Button variant="outline" planned={t(windowKeys.appeal.resubmit, "Replace a document and resubmit the appeal")}>
          {t(windowKeys.appeal.resubmit.label, "Resubmit")}
        </Button>
      </section>
      <div className="grid grid-cols-1 gap-4 @3xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <div className="flex flex-col gap-4">
          <Documents gmb={gmb} />
          <section className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3">
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.appeal.sent, "Appeals")}</h3>
            {mail.status === "loading" && <Skeleton className="h-12" />}
            {mail.status === "error" && <ErrorState failure={mail.failure} onRetry={mail.reload} />}
            {mail.status === "ok" && rest.length === 0 && <p className="text-sm text-ink-soft">{t(windowKeys.appeal.sent.none, "No appeal in Google's mail.")}</p>}
            <MailLines lines={rest} />
          </section>
        </div>
        <div className="flex flex-col gap-4">
          <section className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3">
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.appeal.linked, "Linked to it")}</h3>
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

/** Google's latest decision, and the reasons it gives. */
function Decision({ l }: { l: MailLine }) {
  const locale = useLocale();
  return (
    <>
      <ViewLink to={{ view: "email", params: { case: l.case_id } }} className="rounded-sm text-sm font-medium text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
        {l.summary} · {formatDateTime(l.at, locale)}
      </ViewLink>
      {l.reasons.map((r) => (
        <p key={r} className="text-sm break-words text-accent-error">
          “{r}”
        </p>
      ))}
    </>
  );
}

/** What is kept for the GBP: what it gave Google before, and what its appeal wants. */
function Documents({ gmb }: { gmb: Gmb }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const documents = useDocuments(gmb.id);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<GmbDocument | null>(null);
  const add = async (files: File[]) => {
    setBusy(true);
    try {
      for (const f of files) await keepDocument(gmb.id, f, null);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      documents.reload();
    }
  };
  return (
    <section className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3">
      <header className="flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold text-ink">{t(windowKeys.appeal.documents, "Documents")}</h3>
        <AttachFiles multiple busy={busy} onFiles={(files) => void add(files)} />
      </header>
      <p className="rounded-md bg-hover px-3 py-2 font-mono text-xs break-words text-ink">{[t(windowKeys.appeal.documents.profile, "profile"), gmb.name, gmb.address].filter(Boolean).join(" · ")}</p>
      {documents.status === "loading" && <Skeleton className="h-12" />}
      {documents.status === "error" && <ErrorState failure={documents.failure} onRetry={documents.reload} />}
      {documents.status === "ok" && documents.data.documents.length === 0 && <p className="text-sm text-ink-soft">{t(windowKeys.appeal.documents.none, "None kept yet: a Kbis, a bill, a photo of the sign.")}</p>}
      {documents.status === "ok" && (
        <ul className="flex flex-col divide-y divide-border">
          {documents.data.documents.map((d) => (
            <li key={d.id} className="flex items-center gap-2 py-1.5">
              <span className="flex min-w-0 flex-1 flex-col">
                <button type="button" onClick={() => void saveDocument(gmb.id, d.id).catch((e: unknown) => notifyFailure(e, t))} className="max-w-full self-start truncate rounded-sm text-sm text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
                  {d.name}
                </button>
                <span className="truncate text-xs text-ink-soft">{[d.given_for, fileSize(d.size), formatDateTime(d.added_at, locale)].filter(Boolean).join(" · ")}</span>
              </span>
              <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.appeal.documents.remove, "Remove {name}", { name: d.name })} onClick={() => setRemoving(d)}>
                <Trash2 aria-hidden />
              </Button>
            </li>
          ))}
        </ul>
      )}
      {removing !== null && (
        <ConfirmDialog
          open
          tone="error"
          onOpenChange={(open) => !open && setRemoving(null)}
          title={t(windowKeys.appeal.documents.confirm, "Remove {name} for good?", { name: removing.name })}
          description={t(windowKeys.appeal.documents.confirm.why, "What it was given as is not given any more.")}
          confirmLabel={t(windowKeys.appeal.documents.confirm.yes, "Remove")}
          onConfirm={async () => {
            await removeDocument(gmb.id, removing.id).finally(documents.reload);
          }}
        />
      )}
    </section>
  );
}
