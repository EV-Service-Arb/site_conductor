"use client";

import { Badge, Button, NativeSelect, NativeSelectGroup, NativeSelectOption, Skeleton, cn, useUndo } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";
import { Check, ChevronRight, ExternalLink, RotateCcw, ShieldAlert, SlidersHorizontal, TriangleAlert } from "lucide-react";
import { type ReactNode, useId, useState } from "react";

import { useNow } from "@/entities/alerts";
import { openWindow, under, useGmails } from "@/entities/gmb";
import { KINDS, KIND_NAME, Standing, TOPIC_NAME, TopicBadge, WHO_NAME, correctMail, editCase, mail, subjectOf, unnamed, useOpenThread, whyText } from "@/entities/mail";
import { Mend, Reply, WhichGbp } from "@/features/answer-mail";
import { api } from "@/shared/api";
import { type Gmails, type Gmb, type MailCaseDetail, type MailKind, type MailMessage, type MailTopic, mailTopicValues } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { when } from "@/shared/lib/time";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { Gone } from "./gone";

/** A case whole: why it is the operator's, what was said, what Google asks, and the reply. */
export function CaseDetail({ id }: { id: number }) {
  const gmails = useGmails();
  const read = useResource(api, "/mail/cases/{id}", { params: { path: { id } } });
  if (read.status === "error")
    return (
      <div className="p-5">
        <ErrorState failure={read.failure} onRetry={read.reload} />
      </div>
    );
  if (read.status === "loading" || gmails.status !== "ok") return <Skeleton className="m-5 h-60" />;
  return <Shown detail={read.data} gmails={gmails.data} reload={read.reload} />;
}

function Shown({ detail, gmails, reload }: { detail: MailCaseDetail; gmails: Gmails; reload: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const { case: c, messages } = detail;
  const gmail = gmails.gmails.find((g) => g.id === c.gmail_id);
  const gbp = gmails.gmbs.find((g) => g.id === c.gmb_id);
  const thread = useOpenThread();
  const [correcting, setCorrecting] = useState(false);
  const push = useUndo();
  const done = async (to: boolean) => {
    const set = (to: boolean) => editCase(c.id, { done: to }).finally(reload);
    const subject = subjectOf(c, t);
    try {
      await set(to);
      push({ label: to ? t(mail.done.label, "{subject} marked done", { subject }) : t(mail.reopen.label, "{subject} reopened", { subject }), undo: async () => void (await set(!to)), redo: async () => void (await set(to)) });
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  // removed with its cases, here or in another tab: the read that follows answers 404
  if (gmail === undefined) return <Skeleton className="m-5 h-60" />;
  return (
    <article className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-4 py-5 @3xl:px-7">
      <header className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold break-words text-ink">{subjectOf(c, t)}</h2>
        <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-ink-soft">
          <TopicBadge topic={c.topic} />
          <Standing c={c} />
          {gbp !== undefined ? <GbpLink gbp={gbp} /> : unnamed(c) ? <WhichGbp c={c} onChanged={reload} className="max-w-60" /> : <span>{t(mail.noGbp, "no GBP known")}</span>}
          <span className="min-w-0 truncate">{gmail.gmail}</span>
          {c.numbers.length > 0 && <span className="font-mono text-xs">{c.numbers.join(" · ")}</span>}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" size={button("sm")} disabled={thread.opening !== null} onClick={() => void thread.open(c.gmail_id, c.thread)} title={t(mail.thread.where, "In {gmail}'s running browser", { gmail: gmail.gmail })}>
            <ExternalLink aria-hidden />
            {t(mail.thread.open, "Open thread")}
          </Button>
          {c.state.is !== "done" && (
            <Button variant="outline" size={button("sm")} onClick={() => void done(true)} title={t(mail.done.means, "Until Google writes again; its draft is let go")}>
              <Check aria-hidden />
              {t(mail.done, "Mark done")}
            </Button>
          )}
          {c.state.is === "done" && c.state.by_hand && (
            <Button variant="outline" size={button("sm")} onClick={() => void done(false)}>
              <RotateCcw aria-hidden />
              {t(mail.reopen, "Reopen")}
            </Button>
          )}
          <Button variant="ghost" size={button("sm")} aria-expanded={correcting} onClick={() => setCorrecting(!correcting)}>
            <SlidersHorizontal aria-hidden />
            {t(mail.correct, "Correct")}
          </Button>
        </div>
      </header>
      {correcting && <Correct detail={detail} gbps={under(gmails, c.gmail_id)} reload={reload} />}
      {!messages.every((m) => m.sent || m.authentic) && (
        <Note tone="error" icon={<ShieldAlert aria-hidden />}>
          {t(mail.forged, "Not from Google: Gmail could not prove who sent it. Open nothing in it.")}
        </Note>
      )}
      <Why detail={detail} />
      <Timeline messages={messages} reload={reload} />
      {c.asks.length > 0 && c.last.who !== "us" && (
        <section className="flex flex-col gap-2 rounded-lg bg-hover/60 px-4 py-3">
          <h3 className="text-xs text-ink-soft">{t(mail.asks, "Google asks")}</h3>
          <ul className="flex list-disc flex-col gap-1 pl-4 text-sm text-ink">
            {c.asks.map((a) => (
              <li key={a}>{a}</li>
            ))}
          </ul>
        </section>
      )}
      {detail.gone !== null && <Gone gone={detail.gone} gmb={c.gmb_id} />}
      <Reply detail={detail} gmail={gmail.gmail} onChanged={reload} />
    </article>
  );
}

function GbpLink({ gbp }: { gbp: Gmb }) {
  const first = gbp.tabs[0];
  if (first === undefined) throw new Error(`GBP ${gbp.id} has no tabs`);
  return (
    <button type="button" onClick={() => openWindow({ kind: "gbp", gmb: gbp.id, tab: first.tab })} className="flex min-w-0 items-center gap-1 rounded-sm text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
      <span className="truncate">{gbp.name}</span>
      <ChevronRight aria-hidden className="size-3.5 shrink-0" />
    </button>
  );
}

const NOTE = { warn: "border-accent-warn/40 bg-accent-warn/5 [&>svg]:text-accent-warn", error: "border-accent-error/40 bg-accent-error/5 [&>svg]:text-accent-error" };

function Note({ tone, icon, children }: { tone: keyof typeof NOTE; icon: ReactNode; children: ReactNode }) {
  return (
    <section className={cn("flex gap-3 rounded-lg border px-4 py-3 text-sm text-ink [&>svg]:mt-0.5 [&>svg]:size-4 [&>svg]:shrink-0", NOTE[tone])}>
      {icon}
      <div className="flex min-w-0 flex-1 flex-col gap-2">{children}</div>
    </section>
  );
}

/** Why a person decides, and the date Google set. */
function Why({ detail: { case: c } }: { detail: MailCaseDetail }) {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  const why = c.state.is === "needs_you" ? c.state.why : null;
  const deadline = c.state.is === "done" ? null : c.deadline;
  if (why === null && deadline === null) {
    if (c.state.is !== "waiting" || c.said !== "routed") return null;
    const until = when(c.state.until, locale, now);
    return (
      <p className="text-sm text-ink-soft">
        {c.last.who === "us" || c.draft?.state === "sent"
          ? t(mail.routed.pressed, "Asked again. If the specialist is still silent at {until}, it needs you.", { until })
          : t(mail.routed, "A specialist answers by {until}. If they are silent past it, a reply asks again, once.", { until })}
      </p>
    );
  }
  const said = why === null || why.code === "deadline" ? null : whyText(c, why, t, locale, now);
  return (
    <Note tone={deadline !== null || why?.code === "failed" || why?.code === "may_have_gone_out" ? "error" : "warn"} icon={<TriangleAlert aria-hidden />}>
      {deadline !== null && <p className="font-medium">{t(mail.deadline, "Google's deadline: {at}", { at: formatDateTime(deadline, locale) })}</p>}
      {said !== null && <p className="break-words">{said}</p>}
      {why?.code === "stuck" && <Mend />}
      {why?.code === "unread" && why.error !== null && <p className="text-ink-soft">{t(mail.unread.mend, "Read it below and set its kind.")}</p>}
    </Note>
  );
}

const AS_SAID = "as_said";

/** The case's GBP and topic by hand, over what its mail says. */
function Correct({ detail: { case: c }, gbps, reload }: { detail: MailCaseDetail; gbps: readonly Gmb[]; reload: () => void }) {
  const t = useT();
  const id = useId();
  const [busy, setBusy] = useState(false);
  const set = async (edit: Parameters<typeof editCase>[1]) => {
    setBusy(true);
    try {
      await editCase(c.id, edit);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      reload();
    }
  };
  return (
    <div className="grid grid-cols-1 gap-3 rounded-lg border border-border p-4 @2xl:grid-cols-2">
      <div className="flex min-w-0 flex-col gap-1">
        <label htmlFor={`${id}-gbp`} className="text-xs text-ink-soft">
          {t(mail.correct.gbp, "GBP")}
          {c.by_hand.gmb && ` · ${t(mail.message.byHand, "set by hand")}`}
        </label>
        <NativeSelect id={`${id}-gbp`} size="sm" wrapperClassName="w-full" disabled={busy} value={c.gmb_id ?? AS_SAID} onChange={(e) => void set({ gmb: e.target.value === AS_SAID ? null : Number(e.target.value) })}>
          {c.by_hand.gmb ? <NativeSelectOption value={AS_SAID}>{t(mail.correct.asSaid, "As its mail says")}</NativeSelectOption> : c.gmb_id === null && <NativeSelectOption value={AS_SAID}>{t(mail.correct.unknown, "Not known")}</NativeSelectOption>}
          {gbps.map((g) => (
            <NativeSelectOption key={g.id} value={g.id}>
              {g.name}
            </NativeSelectOption>
          ))}
          {/* moved to another gmail since */}
          {c.gmb_id !== null && !gbps.some((g) => g.id === c.gmb_id) && <NativeSelectOption value={c.gmb_id}>{t(mail.correct.elsewhere, "Under another gmail")}</NativeSelectOption>}
        </NativeSelect>
      </div>
      <div className="flex min-w-0 flex-col gap-1">
        <label htmlFor={`${id}-topic`} className="text-xs text-ink-soft">
          {t(mail.correct.topic, "Topic")}
          {c.by_hand.topic && ` · ${t(mail.message.byHand, "set by hand")}`}
        </label>
        <NativeSelect id={`${id}-topic`} size="sm" wrapperClassName="w-full" disabled={busy} value={c.topic} onChange={(e) => void set({ topic: e.target.value === AS_SAID ? null : (e.target.value as MailTopic) })}>
          {c.by_hand.topic && <NativeSelectOption value={AS_SAID}>{t(mail.correct.asSaid, "As its mail says")}</NativeSelectOption>}
          {mailTopicValues.map((topic) => (
            <NativeSelectOption key={topic} value={topic}>
              {TOPIC_NAME[topic](t)}
            </NativeSelectOption>
          ))}
        </NativeSelect>
      </div>
    </div>
  );
}

function Timeline({ messages, reload }: { messages: readonly MailMessage[]; reload: () => void }) {
  const t = useT();
  const last = messages.findLast((m) => !m.sent)?.id;
  return (
    <section className="flex flex-col gap-1">
      <h3 className="pb-1 text-xs text-ink-soft">{t(mail.timeline, "Messages")}</h3>
      <ol className="flex flex-col">
        {messages.map((m) => (
          <Message key={m.id} m={m} opened={m.id === last} reload={reload} />
        ))}
      </ol>
    </section>
  );
}

/** A message in a line; opened, its own words and what it was read as. */
function Message({ m, opened, reload }: { m: MailMessage; opened: boolean; reload: () => void }) {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  const [open, setOpen] = useState(opened);
  const who = m.sent ? WHO_NAME.us(t) : !m.authentic ? (m.from ?? t(mail.message.nobody, "No sender")) : WHO_NAME.google(t);
  return (
    <li className="relative flex flex-col border-l border-border pl-4">
      <span aria-hidden className={cn("absolute top-3 -left-1 size-2 rounded-full", m.sent ? "bg-accent-info" : opened ? "bg-accent-warn" : "bg-ink-soft")} />
      <button type="button" aria-expanded={open} onClick={() => setOpen(!open)} className="flex min-h-8 min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 rounded-sm py-1 text-left text-sm outline-none hover:bg-hover/40 focus-visible:ring-2 focus-visible:ring-ring">
        <time dateTime={m.at} title={formatDateTime(m.at, locale)} className="w-24 shrink-0 font-mono text-xs whitespace-nowrap text-ink-soft tabular-nums">
          {when(m.at, locale, now)}
        </time>
        <span className="shrink-0 text-ink">{who}</span>
        {!m.sent && <Badge tone="neutral">{m.reading === null ? t(mail.message.unread, "Not read") : KIND_NAME[m.reading.kind](t)}</Badge>}
        <span className="min-w-0 flex-1 basis-40 truncate text-ink-soft">{m.reading?.summary ?? m.text.replace(/\s+/g, " ")}</span>
      </button>
      {open && (
        <div className="flex flex-col gap-2 pt-1 pb-3">
          <p className="text-xs break-all text-ink-soft">{[m.from, m.to.join(", ")].filter((x) => x !== null && x !== "").join(" → ")}</p>
          <p className="rounded-lg border border-border px-3 py-2 text-sm break-words whitespace-pre-wrap text-ink">{m.text === "" ? t(mail.message.empty, "No text of its own.") : m.text}</p>
          {!m.sent && <Read m={m} reload={reload} />}
        </div>
      )}
    </li>
  );
}

/** What a message of Google's was read as, a person's to correct. */
function Read({ m, reload }: { m: MailMessage; reload: () => void }) {
  const t = useT();
  const id = useId();
  const [busy, setBusy] = useState(false);
  const set = async (kind: MailKind) => {
    setBusy(true);
    try {
      await correctMail(m.id, { kind });
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      reload();
    }
  };
  const reasons = m.reading?.reasons ?? [];
  return (
    <div className="flex flex-col gap-2 text-xs text-ink-soft">
      {reasons.length > 0 && <p className="break-words">{t(mail.message.reasons, "Reasons: {reasons}", { reasons: reasons.join(" · ") })}</p>}
      {m.reading?.kind === "unclear" && <p>{t(mail.message.unclear, "What it says could not be told: it is answered by asking for the goal again. Set its kind if you can tell.")}</p>}
      {m.read_error !== null && <p className="break-words text-accent-error">{m.read_error}</p>}
      <span className="flex flex-wrap items-center gap-2">
        <label htmlFor={id}>{t(mail.message.kind, "Kind")}</label>
        <NativeSelect id={id} size="sm" wrapperClassName="w-56" disabled={busy} value={m.reading?.kind ?? ""} onChange={(e) => void set(e.target.value as MailKind)}>
          {m.reading === null && (
            <NativeSelectOption value="" disabled>
              {t(mail.message.unread, "Not read")}
            </NativeSelectOption>
          )}
          {KINDS.map(({ topic, kinds }) => (
            <NativeSelectGroup key={topic ?? ""} label={topic === null ? t(mail.message.any, "Of any topic") : TOPIC_NAME[topic](t)}>
              {kinds.map((k) => (
                <NativeSelectOption key={k} value={k}>
                  {KIND_NAME[k](t)}
                </NativeSelectOption>
              ))}
            </NativeSelectGroup>
          ))}
        </NativeSelect>
        {m.read_by === "person" && <span>{t(mail.message.byHand, "set by hand")}</span>}
      </span>
    </div>
  );
}
