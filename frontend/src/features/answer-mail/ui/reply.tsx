"use client";

import { Button, ConfirmDialog, Input, Spinner, Textarea, cn, useBinding, useUndo } from "@evinvest/uikit";
import { PenLine, RotateCcw, Send, X } from "lucide-react";
import { type ReactNode, useId, useRef, useState } from "react";

import { useNow } from "@/entities/alerts";
import { AttachFiles, encoded, fileSize, useDocuments, useGmails } from "@/entities/gmb";
import { beginDraft, dismissDraft, keepDraft, mail, restoreDraft, sendDraft, subjectOf, writeDraft } from "@/entities/mail";
import { useSpendConfirm, useTokens } from "@/entities/tokens";
import type { Gmb, MailCaseDetail, MailDraft, MailSend } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { when } from "@/shared/lib/time";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { PickDocument } from "./pick-document";

interface Props {
  detail: MailCaseDetail;
  /** The case's gmail: what a reply goes from. */
  gmail: string;
  onChanged: () => void;
}

/** A write's busy flag and its failure told; what it changed is read again either way. */
function useAct(onChanged: () => void) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const act = async (write: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await write();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      onChanged();
    }
  };
  return { busy, act };
}

/** The case's reply, written, edited and sent by its operator; `null` where nothing takes one. */
export function Reply({ detail, gmail, onChanged }: Props) {
  const { draft, state } = detail.case;
  const none = detail.recipient === null ? null : <Write detail={detail} onChanged={onChanged} />;
  if (draft === null) return none;
  switch (draft.state) {
    // read otherwise since, or not going by itself: its operator's
    case "held":
    case "proposed":
    case "failed":
      // nothing to edit: none was written, it does not open, or its thread is done without it
      if (draft.body === null || (draft.state === "failed" && draft.body === "") || state.is === "done") return none;
      return <Editor key={draft.id} draft={draft} written={draft.body} detail={detail} gmail={gmail} onChanged={onChanged} />;
    case "sending":
    case "sent":
      return <HandedOver draft={draft} detail={detail} />;
    case "dismissed":
    case "superseded":
      return none;
  }
}

function Box({ title, note, children }: { title: string; note?: ReactNode; children: ReactNode }) {
  return (
    <section data-reply tabIndex={-1} className="flex flex-col gap-3 rounded-lg border border-border p-4 outline-none">
      <header className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h3 className="text-sm font-semibold text-ink">{title}</h3>
        {note && <span className="min-w-0 text-xs text-ink-soft">{note}</span>}
      </header>
      {children}
    </section>
  );
}

/** Writes a reply, or another in place of the one there, as the instruction says. */
function Write({ detail, onChanged, over = false, onDone }: { detail: MailCaseDetail; onChanged: () => void; over?: boolean; onDone?: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const control = useControlSize();
  const id = useId();
  const [instruction, setInstruction] = useState("");
  const { busy, act } = useAct(onChanged);
  const held = useTokens();
  const spend = useSpendConfirm();
  const { own, reply } = detail.cost;
  const stuck = detail.case.stuck ?? (!own && held.status === "ok" && held.data.balance <= 0 ? t(mail.write.broke, "No tokens left") : null);
  const write = () =>
    act(async () => {
      await writeDraft(detail.case.id, instruction.trim() === "" ? null : instruction.trim());
      onDone?.();
    });
  const verb = over ? t(mail.write.again, "Rewrite") : t(mail.write, "Write a reply");
  const cost = own ? t(mail.write.own, "on your own model") : reply === null ? null : t(mail.write.cost, "≈ {n} tokens", { n: reply });
  const form = (
    <form
      className="flex flex-wrap items-center gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        void spend.ask(reply ?? undefined, write);
      }}
    >
      <label htmlFor={id} className="sr-only">
        {t(mail.write.instruction, "What it should say")}
      </label>
      <Input id={id} size={control} value={instruction} disabled={busy || stuck !== null} onChange={(e) => setInstruction(e.target.value)} placeholder={over ? t(mail.write.change, "What to change, optional") : t(mail.write.say, "What it should say, optional")} className="min-w-48 flex-1" />
      <Button type="submit" variant="outline" size={button("sm")} disabled={busy || held.status !== "ok"} {...(stuck !== null && { disabledReason: stuck })}>
        {busy ? <Spinner aria-hidden /> : <PenLine aria-hidden />}
        {busy ? t(mail.write.busy, "Writing") : [verb, stuck === null && cost].filter(Boolean).join(" · ")}
      </Button>
      {spend.dialog}
    </form>
  );
  if (over) return form;
  const { draft } = detail.case;
  const dismissed = draft?.state === "dismissed" && draft.error === null ? draft : null;
  return (
    <Box title={t(mail.reply, "Reply")}>
      {form}
      <div className="flex flex-wrap gap-2">
        <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => void act(() => beginDraft(detail.case.id))}>
          {t(mail.write.own.self, "Write it yourself")}
        </Button>
        {dismissed !== null && (
          <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => void act(() => restoreDraft(dismissed.id))}>
            <RotateCcw aria-hidden />
            {t(mail.draft.restore, "Bring the dismissed draft back")}
          </Button>
        )}
      </div>
    </Box>
  );
}

/** Each reply's files while its GBP is not known, by the message it answers: kept while another thread is read, gone with the tab. */
const attached = new Map<number, readonly File[]>();

/** What goes with a reply. */
interface Attached {
  names: readonly string[];
  size: number;
  /** Not all of it is known yet. */
  reading: boolean;
  send: () => Promise<Pick<MailSend, "files" | "documents">>;
  sent: () => void;
}

/** `written`: its text. */
type Written = Props & { draft: MailDraft; written: string };

function Editor(props: Written) {
  const gmails = useGmails();
  const gmb = gmails.status === "ok" ? gmails.data.gmbs.find((g) => g.id === props.detail.case.gmb_id) : undefined;
  return gmb === undefined ? <WithFiles {...props} /> : <WithDocuments {...props} gmb={gmb} />;
}

function WithFiles(props: Written) {
  const answers = props.detail.case.last.id;
  const [files, setFiles] = useState(attached.get(answers) ?? []);
  const keep = (next: readonly File[]) => {
    attached.set(answers, next);
    setFiles(next);
  };
  const size = files.reduce((n, f) => n + f.size, 0);
  return (
    <Form {...props} attached={{ names: files.map((f) => f.name), size, reading: false, send: async () => ({ files: await Promise.all(files.map(encoded)), documents: [] }), sent: () => keep([]) }}>
      <Files files={files} size={size} max={props.detail.files_max} onChange={keep} />
    </Form>
  );
}

/** The documents of its GBP's picked for a reply beside those given, by the message it answers; kept in this browser. */
function usePicked(answers: number) {
  const key = `sa:mail-documents:${answers}`;
  const [ids, setIds] = useState<readonly number[]>(() => JSON.parse(localStorage.getItem(key) ?? "[]") as number[]);
  const keep = (next: readonly number[]) => {
    if (next.length === 0) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(next));
    setIds(next);
  };
  return [ids, keep] as const;
}

function WithDocuments({ gmb, ...props }: Written & { gmb: Gmb }) {
  const { case: c, files_max: max } = props.detail;
  const documents = useDocuments(gmb.id);
  const [picked, setPicked] = usePicked(c.last.id);
  const kept = documents.status === "ok" ? documents.data.documents : [];
  const asked = [...new Set(c.wanted.flatMap((w) => w.document ?? []))];
  const given = kept.filter((d) => asked.includes(d.id));
  const extra = kept.filter((d) => picked.includes(d.id) && !given.includes(d));
  const all = [...given, ...extra];
  const size = all.reduce((n, d) => n + d.size, 0);
  return (
    <Form {...props} attached={{ names: all.map((d) => d.name), size, reading: documents.status !== "ok" || given.length < asked.length, send: () => Promise.resolve({ files: [], documents: all.map((d) => d.id) }), sent: () => setPicked([]) }}>
      {documents.status === "error" && <ErrorState failure={documents.failure} onRetry={documents.reload} />}
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <PickDocument gmb={gmb} documents={documents} givenFor={null} onPicked={(added) => setPicked([...new Set([...picked, ...added.map((d) => d.id)])])} />
        {given.map((d) => (
          <Chip key={d.id} name={d.name} size={d.size} />
        ))}
        {extra.map((d) => (
          <Chip key={d.id} name={d.name} size={d.size} onRemove={() => setPicked(picked.filter((id) => id !== d.id))} />
        ))}
        {all.length > 0 && <Total size={size} max={max} />}
      </div>
    </Form>
  );
}

function Chip({ name, size, onRemove }: { name: string; size: number; onRemove?: () => void }) {
  const t = useT();
  return (
    <span className="flex max-w-full items-center gap-1 rounded-md bg-hover py-0.5 pr-1 pl-2 text-xs text-ink">
      <span className="truncate">{name}</span>
      <span className="shrink-0 text-ink-soft tabular-nums">{fileSize(size)}</span>
      {onRemove && (
        <button type="button" aria-label={t(mail.files.remove, "Take {name} off", { name })} onClick={onRemove} className="rounded-sm p-0.5 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
          <X aria-hidden className="size-3" />
        </button>
      )}
    </span>
  );
}

function Total({ size, max }: { size: number; max: number }) {
  const t = useT();
  return <span className={cn("text-xs tabular-nums", size > max ? "text-accent-error" : "text-ink-soft")}>{t(mail.files.size, "{size} of {max}", { size: fileSize(size), max: fileSize(max) })}</span>;
}

function Form({ draft, written, detail, gmail, onChanged, attached: { names, size, reading, send: payload, sent }, children }: Written & { attached: Attached; children: ReactNode }) {
  const t = useT();
  const button = useButtonSize();
  const { recipient, files_max: max } = detail;
  const { busy, act } = useAct(onChanged);
  const push = useUndo();
  const [body, setBody] = useState(written);
  const [confirming, setConfirming] = useState(false);
  const [rewriting, setRewriting] = useState(false);
  const refused =
    recipient === null ? t(mail.send.noRecipient, "Nothing here takes a reply") : body.trim() === "" ? t(mail.send.empty, "The reply is empty") : size > max ? t(mail.send.tooLarge, "The files are over {max} in all", { max: fileSize(max) }) : reading ? t(mail.files.reading, "Its documents are being read") : null;
  useBinding({ on: { key: "Enter" }, id: "mail-send", run: { label: t(mail.send.key, "Send the reply"), run: refused === null && !busy ? () => setConfirming(true) : null } });
  // the button that took the focus is not to be disabled under its press: no `act`
  const keep = () => {
    if (body.trim() !== "" && body !== written)
      keepDraft(draft.id, body)
        .catch((e: unknown) => notifyFailure(e, t))
        .finally(onChanged);
  };
  const n = names.length;
  const unmet = detail.case.wanted.filter((w) => !w.given).map((w) => w.item);
  const lacking = n === 0 && unmet.length > 0;
  const send = () =>
    act(async () => {
      await sendDraft(draft.id, { body, as_is: lacking, ...(await payload()) });
      sent();
    });
  const dismiss = () =>
    act(async () => {
      const again = () => dismissDraft(draft.id);
      await again();
      push({
        label: t(mail.draft.dismissed, "{subject}: draft dismissed", { subject: subjectOf(detail.case, t) }),
        undo: async () => void (await restoreDraft(draft.id).finally(onChanged)),
        redo: async () => void (await again().finally(onChanged)),
      });
    });
  return (
    <Box title={t(mail.draft, "Draft")} note={recipient !== null && t(mail.draft.route, "{gmail} → {to}", { gmail, to: recipient })}>
      <Textarea rows={Math.min(14, Math.max(4, body.split("\n").length + 1))} value={body} onChange={(e) => setBody(e.target.value)} onBlur={keep} aria-label={t(mail.draft.text, "The reply's text")} className="text-sm" />
      {children}
      <div className="flex flex-wrap items-center gap-2">
        <Button size={button("sm")} disabled={busy} {...(refused !== null && { disabledReason: refused })} onClick={() => setConfirming(true)}>
          <Send aria-hidden />
          {n === 0 ? t(mail.send.google, "Send to Google") : t(mail.send.files, "Send to Google · {n, plural, one {# file} other {# files}}", { n })}
        </Button>
        {recipient !== null && (
          <Button variant="outline" size={button("sm")} aria-expanded={rewriting} onClick={() => setRewriting(!rewriting)}>
            {t(mail.write.again, "Rewrite")}
          </Button>
        )}
        <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => void dismiss()}>
          {t(mail.draft.dismiss, "Dismiss draft")}
        </Button>
      </div>
      {rewriting && recipient !== null && <Write over detail={detail} onChanged={onChanged} onDone={() => setRewriting(false)} />}
      {recipient !== null && (
        <ConfirmDialog
          open={confirming}
          onOpenChange={setConfirming}
          title={t(mail.send.confirm, "Send the reply to {to}?", { to: recipient })}
          description={[
            n === 0 ? t(mail.send.confirm.bare, "From {gmail}, with no files.", { gmail }) : t(mail.send.confirm.with, "From {gmail}, with {files}.", { gmail, files: names.join(", ") }),
            unmet.length > 0 && t(mail.send.confirm.lacking, "Still asked for, unless the text answers it: {needs}.", { needs: unmet.join("; ") }),
            t(mail.send.confirm.final, "It cannot be taken back."),
          ]
            .filter(Boolean)
            .join(" ")}
          confirmLabel={lacking ? t(mail.send.asIs, "Send as it is") : n === 0 ? t(mail.send, "Send") : t(mail.send.n, "Send · {n, plural, one {# file} other {# files}}", { n })}
          labels={{ cancel: t(mail.cancel, "Cancel") }}
          onConfirm={send}
        />
      )}
    </Box>
  );
}

function Files({ files, size, max, onChange }: { files: readonly File[]; size: number; max: number; onChange: (files: readonly File[]) => void }) {
  const input = useRef<HTMLInputElement>(null);
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm">
      <AttachFiles multiple input={input} onFiles={(picked) => onChange([...files, ...picked])} />
      {files.map((f, i) => (
        <Chip
          key={i}
          name={f.name}
          size={f.size}
          onRemove={() => {
            // the last one's button goes with it
            if (i === files.length - 1) input.current!.focus();
            onChange(files.filter((_, x) => x !== i));
          }}
        />
      ))}
      {files.length > 0 && <Total size={size} max={max} />}
    </div>
  );
}

function Text({ children }: { children: string }) {
  return <p className="rounded-md bg-hover/60 px-3 py-2 text-sm break-words whitespace-pre-wrap text-ink">{children}</p>;
}

/** Going by itself: when, and how to stop it. */
export function Held({ draft, detail, gmail, onChanged }: Props & { draft: MailDraft }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const now = useNow(1000);
  const { busy, act } = useAct(onChanged);
  const { send_at: at, body } = draft;
  if (at === null || body === null) throw new Error(`draft ${draft.id} is held until no moment, or does not open`);
  const left = Math.ceil((Date.parse(at) - now.getTime()) / 1000);
  const stop = () => act(() => keepDraft(draft.id, body));
  return (
    <Box title={t(mail.held, "Goes by itself at {at}", { at: when(at, locale, now) })} note={detail.recipient !== null && t(mail.draft.route, "{gmail} → {to}", { gmail, to: detail.recipient })}>
      <div className="flex flex-wrap items-center gap-3">
        <Button size={button("sm")} variant="destructive" disabled={busy} onClick={() => void stop()}>
          {t(mail.held.stop, "Stop, keep it for me")}
        </Button>
        <span role="timer" className="font-mono text-sm text-accent-info tabular-nums">
          {left > 0 ? t(mail.held.left, "in {left}", { left: `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}` }) : t(mail.held.due, "due")}
        </span>
      </div>
      <Text>{body}</Text>
    </Box>
  );
}

/** Out of the operator's hands: sent, being sent, or cut off while it was. */
function HandedOver({ draft, detail }: { draft: MailDraft; detail: MailCaseDetail }) {
  const t = useT();
  const locale = useLocale();
  const now = useNow(30_000);
  const to = detail.recipient ?? "Google";
  const { state } = detail.case;
  const unsure = state.is === "needs_you" && state.why.code === "may_have_gone_out";
  const title =
    draft.sent_at !== null
      ? t(mail.sent, "Sent to {to} · {at}", { to, at: when(draft.sent_at, locale, now) })
      : unsure
        ? t(mail.unsure, "May have gone out to {to}", { to })
        : t(mail.sending, "Being sent to {to}", { to });
  return (
    <Box title={title} note={draft.error}>
      {draft.body !== null && <Text>{draft.body}</Text>}
    </Box>
  );
}
