"use client";

import { Button, toast } from "@evinvest/uikit";
import { CircleCheck, CircleDashed, Copy } from "lucide-react";
import { useState } from "react";

import { LANG, googleName, saveDocument, useDocuments, useGmails, useTrade } from "@/entities/gmb";
import { giveMail, mail } from "@/entities/mail";
import type { Gmb, MailCase, MailWanted } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { PickDocument } from "./pick-document";

/** Whether what a case wants is still to be given: answered, it is Google's to say. */
const wants = (c: MailCase) => c.wanted.length > 0 && (c.last.who !== "us" || (c.draft?.needs.length ?? 0) > 0);

/** What Google asks of a case and its reply lacks, each given here or not. */
export function Wanted({ c, onChanged }: { c: MailCase; onChanged: () => void }) {
  const t = useT();
  const gmails = useGmails();
  const gmb = gmails.status === "ok" ? gmails.data.gmbs.find((g) => g.id === c.gmb_id) : undefined;
  if (!wants(c)) return null;
  return (
    <section className="flex flex-col gap-3 rounded-lg bg-hover/60 px-4 py-3">
      <h3 className="flex justify-between gap-2 text-xs text-ink-soft">
        {t(mail.wanted, "Wanted of you")}
        <span className="tabular-nums">{t(mail.wanted.n, "{n} of {all} given", { n: c.wanted.filter((w) => w.given).length, all: c.wanted.length })}</span>
      </h3>
      {gmb === undefined ? <Items c={c} onChanged={onChanged} /> : <ItemsOf gmb={gmb} c={c} onChanged={onChanged} />}
    </section>
  );
}

function ItemsOf({ gmb, c, onChanged }: { gmb: Gmb; c: MailCase; onChanged: () => void }) {
  const documents = useDocuments(gmb.id);
  return (
    <>
      <Items c={c} onChanged={onChanged} of={{ gmb, documents }} />
      <Known gmb={gmb} />
    </>
  );
}

interface Of {
  gmb: Gmb;
  documents: ReturnType<typeof useDocuments>;
}

function Items({ c, onChanged, of }: { c: MailCase; onChanged: () => void; of?: Of }) {
  return (
    <ul className="flex flex-col gap-3">
      {c.wanted.map((w) => (
        <Item key={w.item} w={w} c={c} onChanged={onChanged} {...(of && { of })} />
      ))}
    </ul>
  );
}

function Item({ w, c, onChanged, of }: { w: MailWanted; c: MailCase; onChanged: () => void; of?: Of }) {
  const t = useT();
  const button = useButtonSize();
  const [busy, setBusy] = useState(false);
  const give = async (given: boolean, document: number | null = null) => {
    setBusy(true);
    try {
      await giveMail(c.id, w.item, given, document);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      onChanged();
    }
  };
  const by = of?.documents.status === "ok" ? of.documents.data.documents.find((d) => d.id === w.document) : undefined;
  const Icon = w.given ? CircleCheck : CircleDashed;
  return (
    <li className="flex items-start gap-2 text-sm">
      <Icon aria-hidden className={w.given ? "mt-0.5 size-4 shrink-0 text-positive" : "mt-0.5 size-4 shrink-0 text-accent-warn"} />
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <p className="break-words text-ink">
          <span className="sr-only">{w.given ? t(mail.wanted.given, "Given") : t(mail.wanted.missing, "Not given")}: </span>
          {w.item}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          {w.given ? (
            <>
              {by === undefined ? (
                w.document === null && <span className="text-xs text-ink-soft">{t(mail.wanted.bare, "without a file")}</span>
              ) : (
                <button type="button" onClick={() => void saveDocument(of!.gmb.id, by.id).catch((e: unknown) => notifyFailure(e, t))} className="max-w-full truncate rounded-sm text-xs text-ink-soft underline outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
                  {by.name}
                </button>
              )}
              <Button variant="ghost" size={button("xs")} disabled={busy} onClick={() => void give(false)}>
                {t(mail.wanted.undo, "Take back")}
              </Button>
            </>
          ) : (
            <>
              {of !== undefined && <PickDocument {...of} givenFor={w.item} onPicked={([d]) => give(true, d!.id)} />}
              <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => void give(true)}>
                {t(mail.wanted.mark, "Given without a file")}
              </Button>
            </>
          )}
        </div>
      </div>
    </li>
  );
}

/** What the conductor holds of the GBP, to copy into a reply. */
function Known({ gmb }: { gmb: Gmb }) {
  const t = useT();
  const trade = useTrade(gmb.google_primary ?? gmb.trade, gmb.country?.code ?? null);
  const known = [gmb.phone, gmb.address ?? gmb.hidden_address, gmb.website, trade !== null && gmb.country !== null ? googleName(trade, LANG[gmb.country.code]) : null].filter((x) => x !== null);
  if (known.length === 0) return null;
  return (
    <div className="flex min-w-0 flex-wrap gap-1.5">
      {known.map((text) => (
        <button
          key={text}
          type="button"
          title={t(mail.wanted.copy, "Copy")}
          onClick={() => void navigator.clipboard.writeText(text).then(() => toast(t(mail.wanted.copied, "Copied")))}
          className="flex max-w-full items-center gap-1.5 rounded-md border border-border px-2 py-1 text-xs text-ink outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Copy aria-hidden className="size-3 shrink-0 text-ink-soft" />
          <span className="truncate">{text}</span>
        </button>
      ))}
    </div>
  );
}
