"use client";

import { Button, Checkbox, ConfirmDialog, Input, Spinner, Textarea, cn, useBinding } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";
import { Paperclip, PenLine, Send, X } from "lucide-react";
import { type ReactNode, useId, useState } from "react";

import { useNow } from "@/entities/alerts";
import { dismissDraft, keepDraft, mail, sendDraft, writeDraft } from "@/entities/mail";
import { useSpendConfirm, useTokens } from "@/entities/tokens";
import { api } from "@/shared/api";
import type { MailCaseDetail, MailDraft, MailFile } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { when } from "@/shared/lib/time";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { useDraftCost } from "../model/use-draft-cost";

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

/** The case's reply: written, edited and sent by its operator, or going by itself. */
export function Reply({ detail, gmail, onChanged }: Props) {
  const t = useT();
  const { draft, state } = detail.case;
  const none = detail.recipient === null ? <p className="text-sm text-ink-soft">{t(mail.send.noRecipient, "Nothing here takes a reply")}</p> : <Write detail={detail} onChanged={onChanged} />;
  if (draft === null) return none;
  switch (draft.state) {
    case "held":
      if (state.is === "sending") return <Held draft={draft} detail={detail} gmail={gmail} onChanged={onChanged} />;
    // what it answers was read otherwise since: it goes no more
    // falls through
    case "proposed":
    case "failed":
      // nothing was written: there is nothing to edit
      if (draft.body === "") return none;
      return <Editor key={draft.id} draft={draft} detail={detail} gmail={gmail} onChanged={onChanged} />;
    case "sending":
    case "sent":
      return <HandedOver draft={draft} detail={detail} onChanged={onChanged} />;
    case "dismissed":
    case "superseded":
      return none;
  }
}

function Box({ title, note, children }: { title: string; note?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3 rounded-lg border border-border p-4">
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
  const runs = useDraftCost(detail.messages.map((m) => m.text));
  const write = () =>
    act(async () => {
      await writeDraft(detail.case.id, instruction.trim() === "" ? null : instruction.trim());
      onDone?.();
    });
  const verb = over ? t(mail.write.again, "Rewrite") : t(mail.write, "Write a reply");
  const cost = runs === null ? null : runs.own ? t(mail.write.own, "on your Claude") : runs.cost === undefined ? null : t(mail.write.cost, "≈ {n} tokens", { n: runs.cost });
  const form = (
    <form
      className="flex flex-wrap items-center gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (runs !== null) void spend.ask(runs.own ? 0 : runs.cost, write);
      }}
    >
      <label htmlFor={id} className="sr-only">
        {t(mail.write.instruction, "What it should say")}
      </label>
      <Input id={id} size={control} value={instruction} disabled={busy} onChange={(e) => setInstruction(e.target.value)} placeholder={over ? t(mail.write.change, "What to change, optional") : t(mail.write.say, "What it should say, optional")} className="min-w-48 flex-1" />
      <Button type="submit" variant="outline" size={button("sm")} disabled={busy || runs === null || held.status !== "ok"}>
        {busy ? <Spinner aria-hidden /> : <PenLine aria-hidden />}
        {busy ? t(mail.write.busy, "Writing") : [verb, cost].filter(Boolean).join(" · ")}
      </Button>
      {spend.dialog}
    </form>
  );
  if (over) return form;
  return <Box title={t(mail.reply, "Reply")}>{form}</Box>;
}

function Editor({ draft, detail, gmail, onChanged }: Props & { draft: MailDraft }) {
  const t = useT();
  const button = useButtonSize();
  const { recipient, files_max: max } = detail;
  const { busy, act } = useAct(onChanged);
  const [body, setBody] = useState(draft.body);
  const [have, setHave] = useState<ReadonlySet<string>>(new Set());
  const [files, setFiles] = useState<readonly File[]>([]);
  const [confirming, setConfirming] = useState(false);
  const [rewriting, setRewriting] = useState(false);
  const size = files.reduce((n, f) => n + f.size, 0);
  const lacking = draft.needs.filter((n) => !have.has(n));
  const refused =
    recipient === null ? t(mail.send.noRecipient, "Nothing here takes a reply") : body.trim() === "" ? t(mail.send.empty, "The reply is empty") : size > max ? t(mail.send.tooLarge, "The files are over {max} in all", { max: megabytes(max) }) : null;
  useBinding({ on: { key: "Enter" }, id: "mail-send", run: { label: t(mail.send.key, "Send the reply"), run: refused === null && !busy ? () => setConfirming(true) : null } });
  // the button that took the focus is not to be disabled under its press: no `act`
  const keep = () => {
    if (body.trim() !== "" && body !== draft.body)
      keepDraft(draft.id, body)
        .catch((e: unknown) => notifyFailure(e, t))
        .finally(onChanged);
  };
  const send = () => act(async () => sendDraft(draft.id, body, await Promise.all(files.map(encoded))));
  return (
    <Box title={t(mail.draft, "Draft")} note={recipient !== null && t(mail.draft.route, "{gmail} → {to}", { gmail, to: recipient })}>
      <Textarea rows={Math.min(14, Math.max(4, body.split("\n").length + 1))} value={body} onChange={(e) => setBody(e.target.value)} onBlur={keep} aria-label={t(mail.draft.text, "The reply's text")} className="text-sm" />
      {draft.needs.length > 0 && (
        <fieldset className="flex flex-col gap-2">
          <legend className="pb-2 text-xs text-ink-soft">{t(mail.draft.needs, "Still needs, only you have it: attach it, or say it in the text")}</legend>
          {draft.needs.map((need) => (
            <label key={need} className="flex items-start gap-2 text-sm text-ink">
              <Checkbox
                checked={have.has(need)}
                onCheckedChange={(on) => setHave((was) => new Set(on === true ? [...was, need] : [...was].filter((n) => n !== need)))}
                className="mt-0.5"
              />
              <span className={cn("min-w-0", have.has(need) && "text-ink-soft line-through")}>{need}</span>
            </label>
          ))}
        </fieldset>
      )}
      <Files files={files} size={size} max={max} onChange={setFiles} />
      <div className="flex flex-wrap items-center gap-2">
        <Button size={button("sm")} disabled={busy} {...(refused !== null && { disabledReason: refused })} onClick={() => setConfirming(true)} className="max-w-full">
          <Send aria-hidden />
          <span className="truncate">{recipient === null ? t(mail.send, "Send") : t(mail.send.to, "Send to {to}", { to: recipient })}</span>
        </Button>
        {recipient !== null && (
          <Button variant="outline" size={button("sm")} aria-expanded={rewriting} onClick={() => setRewriting(!rewriting)}>
            {t(mail.write.again, "Rewrite")}
          </Button>
        )}
        <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => void act(() => dismissDraft(draft.id))}>
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
            files.length === 0 ? t(mail.send.confirm.body, "It goes to Google from {gmail}, and cannot be taken back.", { gmail }) : t(mail.send.confirm.files, "It goes to Google from {gmail} with {n} files, and cannot be taken back.", { gmail, n: files.length }),
            lacking.length > 0 ? t(mail.send.confirm.lacking, "Not ticked: {what}.", { what: lacking.join(", ") }) : null,
          ]
            .filter(Boolean)
            .join(" ")}
          confirmLabel={t(mail.send, "Send")}
          labels={{ cancel: t(mail.cancel, "Cancel") }}
          onConfirm={send}
        />
      )}
    </Box>
  );
}

function Files({ files, size, max, onChange }: { files: readonly File[]; size: number; max: number; onChange: (files: readonly File[]) => void }) {
  const t = useT();
  const button = useButtonSize();
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm">
      <Button asChild variant="outline" size={button("sm")}>
        <label className="cursor-pointer">
          <Paperclip aria-hidden />
          {t(mail.files.attach, "Attach files")}
          <input
            type="file"
            multiple
            className="sr-only"
            onChange={(e) => {
              const picked = [...(e.target.files ?? [])];
              e.target.value = "";
              onChange([...files, ...picked]);
            }}
          />
        </label>
      </Button>
      {files.map((f, i) => (
        <span key={i} className="flex max-w-full items-center gap-1 rounded-md bg-hover py-0.5 pr-1 pl-2 text-xs text-ink">
          <span className="truncate">{f.name}</span>
          <span className="shrink-0 text-ink-soft tabular-nums">{megabytes(f.size)}</span>
          <button type="button" aria-label={t(mail.files.remove, "Take {name} off", { name: f.name })} onClick={() => onChange(files.filter((_, x) => x !== i))} className="rounded-sm p-0.5 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
            <X aria-hidden className="size-3" />
          </button>
        </span>
      ))}
      {files.length > 0 && <span className={cn("text-xs tabular-nums", size > max ? "text-accent-error" : "text-ink-soft")}>{t(mail.files.size, "{size} of {max}", { size: megabytes(size), max: megabytes(max) })}</span>}
    </div>
  );
}

const megabytes = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;

async function encoded(file: File): Promise<MailFile> {
  const url = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
  // a kind the browser does not know is bytes, as RFC 2046 has it
  return { name: file.name, type: file.type === "" ? "application/octet-stream" : file.type, base64: url.slice(url.indexOf(",") + 1) };
}

function Text({ children }: { children: string }) {
  return <p className="rounded-md bg-hover/60 px-3 py-2 text-sm break-words whitespace-pre-wrap text-ink">{children}</p>;
}

/** Going by itself: when, and how to stop it; where nothing does, that. */
function Held({ draft, detail, gmail, onChanged }: Props & { draft: MailDraft }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const now = useNow(1000);
  const { busy, act } = useAct(onChanged);
  const auto = useResource(api, "/mail/auto");
  const stays = auto.status === "ok" && !auto.data.by_itself;
  if (draft.send_at === null) throw new Error(`draft ${draft.id} is held until no moment`);
  const left = Math.ceil((Date.parse(draft.send_at) - now.getTime()) / 1000);
  const stop = () => act(() => keepDraft(draft.id, draft.body));
  return (
    <Box title={stays ? t(mail.held.stays, "Held, and stays here") : t(mail.held, "Goes by itself at {at}", { at: when(draft.send_at, locale, now) })} note={detail.recipient !== null && t(mail.draft.route, "{gmail} → {to}", { gmail, to: detail.recipient })}>
      <Text>{draft.body}</Text>
      <div className="flex flex-wrap items-center gap-3">
        <Button size={button("sm")} variant={stays ? "outline" : "destructive"} disabled={busy} onClick={() => void stop()}>
          {stays ? t(mail.held.take, "Take it over") : t(mail.held.stop, "Stop, keep it for me")}
        </Button>
        {stays ? (
          <span className="text-sm text-ink-soft">{t(mail.auto.here, "This conductor sends nothing by itself: production's does.")}</span>
        ) : (
          <span role="timer" className="font-mono text-sm text-accent-info tabular-nums">
            {left > 0 ? t(mail.held.left, "in {left}", { left: `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}` }) : t(mail.held.due, "due")}
          </span>
        )}
      </div>
    </Box>
  );
}

/** Out of the operator's hands: sent, being sent, or cut off while it was. */
function HandedOver({ draft, detail, onChanged }: { draft: MailDraft; detail: MailCaseDetail; onChanged: () => void }) {
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
    <>
      <Box title={title} note={draft.error}>
        <Text>{draft.body}</Text>
      </Box>
      {unsure && detail.recipient !== null && <Write detail={detail} onChanged={onChanged} />}
    </>
  );
}
