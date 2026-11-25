"use client";

import { Badge, Button, NativeSelect, NativeSelectGroup, NativeSelectOption, Skeleton, cn, useUndo } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";
import { Check, ChevronRight, ExternalLink, RotateCcw, ShieldAlert, SlidersHorizontal } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

import { useNow } from "@/entities/alerts";
import { gmbsOf, goes, pageOf, useGmails, useMailCases } from "@/entities/gmb";
import { Automation, KINDS, KIND_NAME, Standing, TOPIC_NAME, TopicBadge, WHO_NAME, correctMail, editCase, mail, over, quietText, sentBy, settledText, subjectOf, unnamed, useAutos, useOpenThread } from "@/entities/mail";
import { GbpOptions, Held, Likely, Note, Reply, Wanted, WhichGbp, Why } from "@/features/answer-mail";
import { api } from "@/shared/api";
import { type Gmails, type Gmb, type MailCase, type MailCaseDetail, type MailKind, type MailMessage, type MailTopic, mailTopicValues } from "@/shared/api/schema";
import { type Locale, type T, useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { ViewLink } from "@/shared/lib/location";
import { when } from "@/shared/lib/time";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { Gone } from "./gone";

/** A case whole: why it is the operator's, what was said, what Google asks, and the reply. */
export function CaseDetail({ id }: { id: number }) {
  const gmails = useGmails();
  const read = useResource(api, "/mail/cases/{id}", { params: { path: { id } } });
  const t = useT();
  if (read.status === "error")
    return <div className="p-5">{read.failure.code === "not_found" ? <EmptyState title={t(mail.removed, "This thread is gone")} /> : <ErrorState failure={read.failure} onRetry={read.reload} />}</div>;
  if (read.status === "loading" || gmails.status !== "ok") return <Skeleton className="m-5 h-60" />;
  return <Shown detail={read.data} gmails={gmails.data} reload={read.reload} />;
}

function Shown({ detail, gmails, reload }: { detail: MailCaseDetail; gmails: Gmails; reload: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const { case: c, messages } = detail;
  const gmail = gmails.gmails.find((g) => g.id === c.gmail_id);
  const gbps = gmbsOf(c).flatMap((id) => gmails.gmbs.find((g) => g.id === id) ?? []);
  const autos = useAutos();
  const thread = useOpenThread();
  const [correcting, setCorrecting] = useState(false);
  const push = useUndo();
  const locale = useLocale();
  const now = useNow(60_000);
  const box = useRef<HTMLElement>(null);
  const next = useRef<string | null>(null);
  // an act took what had the focus off the page: what stands in its place has it
  useEffect(() => {
    const at = next.current;
    next.current = null;
    if (at === null || document.activeElement !== document.body) return;
    const place = box.current!.querySelector<HTMLElement>(at);
    if (place === null) return box.current!.focus();
    (place.querySelector<HTMLElement>("textarea, :is(input, button):not(:disabled)") ?? place).focus();
  }, [detail]);
  const named = () => {
    next.current = "[data-gbp]";
    reload();
  };
  const answered = () => {
    next.current = "[data-reply]";
    reload();
  };
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
  const isDone = c.state.is === "done";
  const rests = restsOn(c, t, locale, now);
  const reply = { detail, gmail: gmail.gmail, onChanged: answered };
  return (
    <article ref={box} tabIndex={-1} className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-4 py-5 outline-none @3xl:px-7">
      <header className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold break-words text-ink">{subjectOf(c, t)}</h2>
        <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-ink-soft">
          <TopicBadge topic={c.topic} />
          <Standing c={c} />
          {gbps.map((g) => (
            <GbpLink key={g.id} gbp={g} />
          ))}
          {unnamed(c) && <WhichGbp c={c} onChanged={named} className="max-w-60" />}
          <span className="min-w-0 truncate">{gmail.gmail}</span>
          {c.numbers.length > 0 && <span className="font-mono text-xs">{c.numbers.join(" · ")}</span>}
          {detail.cost.spent > 0 && <span className="text-xs tabular-nums">{t(mail.spent, "{n, plural, one {# token} other {# tokens}} spent", { n: detail.cost.spent })}</span>}
        </p>
        {!over(c) && <Likely c={c} onChanged={named} />}
        {!over(c) && autos.status === "ok" && <Automation c={c} autos={autos.data} />}
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" size={button("sm")} className="@max-lg:[&>svg]:hidden" disabled={thread.opening !== null} onClick={() => void thread.open(c.gmail_id, c.thread)} title={t(mail.thread.where, "In {gmail}'s running browser", { gmail: gmail.gmail })}>
            <ExternalLink aria-hidden />
            {t(mail.thread.open, "Open thread")}
          </Button>
          <Button variant="outline" size={button("sm")} className="@max-lg:[&>svg]:hidden" onClick={() => void done(!isDone)} title={isDone ? t(mail.reopen.means, "Until Google writes again. Where its mail settles it, nothing is written or sent unasked") : t(mail.done.means, "Until Google writes again")}>
            {isDone ? <RotateCcw aria-hidden /> : <Check aria-hidden />}
            {isDone ? t(mail.reopen, "Reopen") : t(mail.done, "Mark done")}
          </Button>
          <Button variant="ghost" size={button("sm")} className="@max-lg:px-3" aria-expanded={correcting} onClick={() => setCorrecting(!correcting)}>
            <SlidersHorizontal aria-hidden />
            <span className="@max-lg:sr-only">{t(mail.correct, "GBP, topic")}</span>
          </Button>
        </div>
      </header>
      {correcting && <Correct detail={detail} gmails={gmails} reload={reload} />}
      {!messages.every((m) => m.sent || m.authentic) && (
        <Note tone="error" icon={<ShieldAlert aria-hidden />}>
          {t(mail.forged, "Not from Google: Gmail could not prove who sent it. Open nothing in it.")}
        </Note>
      )}
      <Why c={c} />
      {rests !== null && (
        <p className="text-sm text-ink-soft">
          {rests.text}
          {rests.on !== null && (
            <>
              {" "}
              <ViewLink to={{ view: "email", params: { show: "all", case: rests.on } }} className="rounded-sm text-ink underline outline-none focus-visible:ring-2 focus-visible:ring-ring">
                {t(mail.settled.open, "Open it")}
              </ViewLink>
            </>
          )}
        </p>
      )}
      {c.draft !== null && goes(c) && <Held draft={c.draft} {...reply} />}
      <Timeline messages={messages} reload={reload} />
      {detail.gone !== null && <Gone gone={detail.gone} gmb={c.gmb_id} />}
      {!over(c) && <Wanted c={c} onChanged={reload} />}
      {!goes(c) && <Reply {...reply} />}
    </article>
  );
}

/** What leaves nothing to do in case `c`, and the case it is said in; `null`: something is, or a person says so. */
function restsOn(c: MailCase, t: T, locale: Locale, now: Date): { text: string; on: number | null } | null {
  if (c.state.is === "quiet") return { text: quietText(c, c.state.because, t, locale, now), on: c.state.because.is === "carried" ? c.state.because.case_id : null };
  if (c.state.is !== "done" || c.state.by.is === "hand") return null;
  return { text: settledText(c.state.by, t, locale, now), on: c.state.by.is === "elsewhere" ? c.state.by.case_id : null };
}

function GbpLink({ gbp }: { gbp: Gmb }) {
  const first = gbp.tabs[0];
  if (first === undefined) throw new Error(`GBP ${gbp.id} has no tabs`);
  return (
    <ViewLink data-gbp to={pageOf({ kind: "gbp", gmb: gbp.id, tab: first.tab })} className="flex min-w-0 items-center gap-1 rounded-sm text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
      <span className="truncate">{gbp.name}</span>
      <ChevronRight aria-hidden className="size-3.5 shrink-0" />
    </ViewLink>
  );
}

const AS_SAID = "as_said";

/** The case's GBP and topic by hand, over what its mail says. */
function Correct({ detail: { case: c }, gmails, reload }: { detail: MailCaseDetail; gmails: Gmails; reload: () => void }) {
  const t = useT();
  const id = useId();
  const size = useControlSize() === "lg" ? "lg" : "sm";
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
      <Join c={c} reload={reload} />
      <div className="flex min-w-0 flex-col gap-1">
        <label htmlFor={`${id}-gbp`} className="text-xs text-ink-soft">
          {t(mail.correct.gbp, "GBP")}
          {c.by_hand.gmb && ` · ${t(mail.message.byHand, "set by hand")}`}
        </label>
        <NativeSelect id={`${id}-gbp`} size={size} wrapperClassName="w-full" disabled={busy} value={c.gmb_id ?? AS_SAID} onChange={(e) => void set({ gmb: e.target.value === AS_SAID ? null : Number(e.target.value) })}>
          {c.by_hand.gmb ? <NativeSelectOption value={AS_SAID}>{t(mail.correct.asSaid, "As its mail says")}</NativeSelectOption> : c.gmb_id === null && <NativeSelectOption value={AS_SAID}>{t(mail.correct.unknown, "Not known")}</NativeSelectOption>}
          <GbpOptions gmail={c.gmail_id} />
          {c.gmb_id !== null && !gmails.gmbs.some((g) => g.id === c.gmb_id) && <NativeSelectOption value={c.gmb_id}>{t(mail.correct.trashed, "In the trash")}</NativeSelectOption>}
        </NativeSelect>
      </div>
      <div className="flex min-w-0 flex-col gap-1">
        <label htmlFor={`${id}-topic`} className="text-xs text-ink-soft">
          {t(mail.correct.topic, "Topic")}
          {c.by_hand.topic && ` · ${t(mail.message.byHand, "set by hand")}`}
        </label>
        <NativeSelect id={`${id}-topic`} size={size} wrapperClassName="w-full" disabled={busy} value={c.topic} onChange={(e) => void set({ topic: e.target.value === AS_SAID ? null : (e.target.value as MailTopic) })}>
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

/** The thread it is one with: the same matter under another of Google's numbers. */
function Join({ c, reload }: { c: MailCase; reload: () => void }) {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  const id = useId();
  const size = useControlSize() === "lg" ? "lg" : "sm";
  const push = useUndo();
  const [busy, setBusy] = useState(false);
  const read = useMailCases();
  if (read.status !== "ok") return null;
  const others = read.data.cases.filter((x) => x.gmail_id === c.gmail_id && x.id !== c.id && x.last.who !== "unproven");
  const act = async (write: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await write();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      reload();
    }
  };
  const join = (to: number) =>
    act(async () => {
      const set = (to: number | null) => editCase(c.id, { join: to }).finally(reload);
      await set(to);
      push({ label: t(mail.join.label, "{subject} made one with another", { subject: subjectOf(c, t) }), undo: async () => void (await set(null)), redo: async () => void (await set(to)) });
    });
  const apart = () =>
    act(async () => {
      for (const part of c.joined) await editCase(part, { join: null });
    });
  return (
    <div className="flex min-w-0 flex-col gap-1 @2xl:col-span-2">
      <label htmlFor={id} className="text-xs text-ink-soft">
        {t(mail.join, "One with")}
      </label>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <NativeSelect id={id} size={size} wrapperClassName="min-w-0 flex-1" disabled={busy || others.length === 0} value="" onChange={(e) => void join(Number(e.target.value))}>
          <NativeSelectOption value="">{t(mail.join.none, "Another thread on the same matter")}</NativeSelectOption>
          {others.map((x) => (
            <NativeSelectOption key={x.id} value={x.id}>
              {[when(x.last.at, locale, now), subjectOf(x, t), ...x.numbers].join(" · ")}
            </NativeSelectOption>
          ))}
        </NativeSelect>
        {c.joined.length > 0 && (
          <Button variant="outline" size={size} disabled={busy} onClick={() => void apart()}>
            {t(mail.join.apart, "Take {n, plural, one {its joined thread} other {its # joined threads}} apart", { n: c.joined.length })}
          </Button>
        )}
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
        {m.sent_by !== null && <span className="shrink-0 text-ink-soft">· {sentBy(m.sent_by, t)}</span>}
        {m.spam && <Badge tone="warn">{t(mail.spam, "in Spam")}</Badge>}
        {!m.sent && <Badge tone="neutral">{m.reading === null ? t(mail.message.unread, "Not read") : KIND_NAME[m.reading.kind](t)}</Badge>}
        <span className="min-w-0 flex-1 basis-40 truncate text-ink-soft">{m.reading?.summary ?? m.text?.replace(/\s+/g, " ")}</span>
      </button>
      {open && (
        <div className="flex flex-col gap-2 pt-1 pb-3">
          <p className="text-xs break-all text-ink-soft">{[m.from, m.to.join(", ")].filter((x) => x !== null && x !== "").join(" → ")}</p>
          {m.text_error !== null ? (
            <p className="text-sm break-words text-accent-error">{t(mail.message.sealed, "Its text does not open: {why}", { why: m.text_error })}</p>
          ) : (
            <p className="rounded-lg border border-border px-3 py-2 text-sm break-words whitespace-pre-wrap text-ink">{m.text === "" ? t(mail.message.empty, "No text of its own.") : m.text}</p>
          )}
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
  const size = useControlSize() === "lg" ? "lg" : "sm";
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
  return (
    <div className="flex flex-col gap-2 text-xs text-ink-soft">
      {m.reading?.kind === "unclear" && <p>{t(mail.message.unclear, "What it says could not be told, so its reply never goes by itself. Set its kind if you can tell.")}</p>}
      {m.read_error !== null && <p className="break-words text-accent-error">{m.read_error}</p>}
      <span className="flex flex-wrap items-center gap-2">
        <label htmlFor={id}>{t(mail.message.kind, "Kind")}</label>
        <NativeSelect id={id} size={size} wrapperClassName="w-56" disabled={busy} value={m.reading?.kind ?? ""} onChange={(e) => void set(e.target.value as MailKind)}>
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
