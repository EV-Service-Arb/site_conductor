"use client";

import { ChatPanel, ConfirmDialog, Kbd, Spinner, toast } from "@evinvest/uikit";
import { CornerDownRight, Eye, Lock, Sparkles, Upload } from "lucide-react";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import { type AssistantTurn, type Mention, type Turn, clearArchived, deleteChat, putTurns, removeMember, useChat, assistant } from "@/entities/assistant";
import { type Listed, narrowed, useGmails, useGmbTags, usePeeked } from "@/entities/gmb";
import { useMe } from "@/entities/session";
import { ApiError } from "@/shared/api";
import { ROUTES } from "@/shared/config/routes";
import { type T, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

import { asMentions, play, script, suggestions } from "../model/canned";
import { type Display, ask, openChat, overlaid, shown, stop, useAssistant } from "../model/chat";
import { type Confirm, type Controls, ControlsContext } from "./chat-actions";
import { Composer, type ComposerHandle, type Draft, keepDraft } from "./composer";
import { ContextCard, Header } from "./header";
import { AssistantTurnView, UserTurnView } from "./turns";

const CONFIRM = {
  delete: { title: (t) => t(assistant.delete.title, "Delete this chat?"), action: (t) => t(assistant.delete, "Delete") },
  leave: { title: (t) => t(assistant.leave.title, "Leave this chat?"), action: (t) => t(assistant.leave, "Leave") },
  clear: { title: (t) => t(assistant.clear.title, "Clear archived chats?"), action: (t) => t(assistant.history.clear, "Clear") },
} satisfies Record<Confirm["kind"], { title: (t: T) => string; action: (t: T) => string }>;

/** Where full screen was opened from: leaving it goes back there. */
let back: string | null = null;

/**
 * One chat: header, conversation and composer, on any of the three surfaces. `id` is `null` for a chat not started;
 * `onOpen` makes another the surface's.
 */
export function ChatView({ id, display, onOpen, onHide }: { id: string | null; display: Display; onOpen: (id: string | null) => void; onHide: (() => void) | null }) {
  const t = useT();
  const me = useMe();
  const router = useRouter();
  const pathname = usePathname();
  const s = useAssistant();
  const read = useChat(id);
  const chat = read.status === "ok" ? read.data : null;
  const turns = shown(chat, id, s.running);
  const gmails = useGmails();
  const { tags } = useGmbTags();
  const all = gmails.status === "ok" ? narrowed(gmails.data, null, "") : [];
  const subject = useSubject(all);
  const composer = useRef<ComposerHandle>(null);
  const [composed, setComposed] = useState(0);
  const [editing, setEditing] = useState<string | null>(null);
  const [dropping, setDropping] = useState(0);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [context, setContext] = useState(true);
  const seen = useRef(new Set<string>());
  const runningHere = overlaid(chat, id, s.running);
  const streaming = runningHere && (s.running!.answer.state === "working" || s.running!.answer.state === "writing");
  const gone = id !== null && read.status === "ok" && read.fresh && read.data === null && !runningHere;
  const interacts = id === null || chat?.role === "owner" || chat?.role === "member" || (chat === null && runningHere);

  const reload = read.reload;
  useEffect(() => {
    if (chat !== null) seen.current.add(chat.id);
  }, [chat]);
  useEffect(() => {
    // Full screen is a link's page: it says so in place. Beside a page, the chat gives way to a new one.
    if (!gone || display === "full") return;
    if (seen.current.has(id)) toast(t(assistant.gone, "That chat was deleted, or is no longer shared with you"));
    onOpen(null);
  }, [gone, id, display, onOpen, t]);
  const written = s.running?.chat === id ? s.running.written : null;
  useEffect(() => {
    if (written !== null && (chat?.version ?? -1) < written) reload();
  }, [written, chat?.version, reload]);

  const failed = (e: unknown) => {
    if (e instanceof ApiError && e.failure.kind === "conflict") toast.error(t(assistant.conflict, "Someone wrote in this chat meanwhile: it is read again, and your message kept"));
    else notifyFailure(e, t);
    reload();
  };
  const resolve = (mentions: readonly Mention[]): Listed[] => mentions.flatMap((m) => all.filter((l) => l.gmb.id === m.id));
  const send = (draft: Draft, from = turns.length) => {
    const named = [...(subject && !draft.subjectOff ? asMentions([subject]) : []), ...draft.mentions].filter((m, i, xs) => xs.findIndex((x) => x.id === m.id) === i);
    const answer = script(draft.text, resolve(named), all, tags, t);
    const asked = ask(chat, from, { text: draft.text, mentions: named, files: draft.files }, (put, signal) => play(answer, s.approval, put, signal), (e) => {
      if (e instanceof ApiError && e.failure.kind === "conflict") toast.error(t(assistant.lost, "Someone wrote in this chat meanwhile: the answer was not saved"));
      else notifyFailure(e, t);
    });
    if (asked.id !== id) onOpen(asked.id);
    asked.written.then(reload, (e: unknown) => {
      keepDraft(id, draft);
      if (asked.id !== id) onOpen(id);
      setComposed((n) => n + 1);
      failed(e);
    });
  };
  const again = (at: number, text: string) => {
    const asked = turns[at];
    if (asked?.role !== "user") throw new Error("a question is re-asked from its own turn");
    setEditing(null);
    composer.current?.focus();
    send({ text, mentions: asked.mentions, files: asked.files, subjectOff: true }, at);
  };
  const change = (at: number, turn: AssistantTurn) => {
    if (chat === null) throw new Error("only a chat read is changed");
    void putTurns(chat.id, chat.version, at, [turn, ...chat.turns.slice(at + 1)]).then(reload, failed);
  };

  const controls: Controls = {
    display,
    onDisplay: (chatId, to) => {
      if (to === display && chatId === id) return;
      if (to === "full") {
        if (display !== "full") {
          const here = new URL(window.location.href);
          here.searchParams.delete("assistant"); // the display chosen on the way back is the one shown
          back = `${pathname}${here.search}`;
        }
        router.push(`${ROUTES.assistant}?chat=${chatId}`);
      } else {
        openChat(chatId, to);
        if (display === "full") {
          router.push(back ?? "/");
          back = null;
        }
      }
    },
    onConfirm: setConfirm,
    onChanged: reload,
  };
  const confirmed = async (c: Confirm) => {
    if (c.kind === "clear") await clearArchived();
    else if (c.kind === "delete") await deleteChat(c.chat.id);
    else await removeMember(c.chat.id, me.user_id);
    if (c.kind === "clear" ? chat !== null && chat.archived_at !== null : c.chat.id === id) onOpen(null);
    reload();
  };

  const header = <Header chat={chat} turns={turns} onOpen={onOpen} onHide={onHide} onRenamed={() => composer.current?.focus()} context={display === "full" ? context : null} onContext={() => setContext((c) => !c)} />;
  const notShared = gone && display === "full";
  const loading = id !== null && chat === null && !runningHere && !gone;
  // Gone beside a page: blank for the moment until the effect above opens a new chat.
  const blank = gone || loading;
  const conversation = (
    <ChatPanel
      streaming={streaming}
      labels={{ jump: t(assistant.jump, "Jump to latest"), log: t(assistant.log, "Conversation") }}
      empty={
        notShared ? (
          <Blocked icon={Lock} text={t(assistant.notShared, "This chat isn't shared with you")} />
        ) : loading ? (
          read.status === "error" ? <Blocked icon={Lock} text={t(assistant.failed, "This chat could not be read")} /> : <Spinner className="m-auto size-5" />
        ) : (
          <Empty subject={subject} onAsk={(text) => send({ text, mentions: [], files: [], subjectOff: false })} />
        )
      }
      composer={
        blank ? null : interacts ? (
          <Composer
            key={`${id ?? ""}:${composed}`}
            handle={composer}
            chat={id}
            subject={subject}
            streaming={streaming}
            approval={s.approval}
            onSend={send}
            onStop={stop}
            onEditLast={() => {
              const asked = turns.findLast((x) => x.role === "user");
              if (asked) setEditing(asked.id);
            }}
          />
        ) : (
          <p className="flex h-11 items-center justify-center gap-2 rounded-2xl bg-secondary text-body-s text-ink-soft">
            <Eye aria-hidden className="size-4" />
            {t(assistant.viewOnly, "View only · shared by {name}", { name: chat!.owner.name })}
          </p>
        )
      }
    >
      {blank ? null : turns.map((turn: Turn, i) =>
        turn.role === "user" ? (
          <UserTurnView
            key={turn.id}
            turn={turn}
            editing={editing === turn.id}
            onEditing={
              interacts
                ? (on) => {
                    setEditing(on ? turn.id : null);
                    if (!on) composer.current?.focus();
                  }
                : null
            }
            onEdit={(text) => again(i, text)}
          />
        ) : (
          <AssistantTurnView
            key={turn.id}
            turn={turn}
            last={i === turns.length - 1}
            onChange={interacts && !(runningHere && i >= s.running!.from) ? (next) => change(i, next) : null}
            onRetry={interacts ? () => again(i - 1, (turns[i - 1] as { text: string }).text) : null}
          />
        ),
      )}
    </ChatPanel>
  );

  return (
    <ControlsContext.Provider value={controls}>
      <div
        className="relative flex min-h-0 flex-1 flex-col"
        onDragEnter={(e) => interacts && e.dataTransfer.types.includes("Files") && setDropping((n) => n + 1)}
        onDragLeave={(e) => interacts && e.dataTransfer.types.includes("Files") && setDropping((n) => n - 1)}
        onDragOver={(e) => interacts && e.dataTransfer.types.includes("Files") && e.preventDefault()}
        onDrop={(e) => {
          if (!interacts || !e.dataTransfer.types.includes("Files")) return;
          e.preventDefault();
          setDropping(0);
          composer.current?.attach([...e.dataTransfer.files]);
        }}
      >
        {header}
        {display === "full" ? (
          <div className="flex min-h-0 flex-1 gap-6 px-6 pb-4">
            <span className="hidden w-64 shrink-0 xl:block" />
            <div className="mx-auto flex min-h-0 w-full max-w-170 flex-1 flex-col">{conversation}</div>
            {context ? <ContextCard turns={turns} /> : <span className="hidden w-64 shrink-0 xl:block" />}
          </div>
        ) : (
          conversation
        )}
        {dropping > 0 && (
          <div className="pointer-events-none absolute inset-2 z-20 flex flex-col items-center justify-center gap-2 rounded-panel border-2 border-dashed border-ring bg-card/90 text-body text-ink">
            <Upload aria-hidden className="size-6 text-ink-soft" />
            {t(assistant.drop, "Drop to attach")}
          </div>
        )}
      </div>
      <ConfirmDialog
        open={confirm !== null}
        onOpenChange={(open) => {
          if (!open) setConfirm(null);
        }}
        tone="error"
        title={CONFIRM[confirm?.kind ?? "delete"].title(t)}
        description={confirm === null || confirm.kind === "clear" ? t(assistant.clear.description, "Your archived chats are deleted; the archived ones shared with you, left.") : (confirm.chat.title ?? t(assistant.newChat, "New chat"))}
        confirmLabel={CONFIRM[confirm?.kind ?? "delete"].action(t)}
        labels={{ cancel: t(assistant.edit.cancel, "Cancel") }}
        onConfirm={() => confirmed(confirm!)}
      />
    </ControlsContext.Provider>
  );
}

function Blocked({ icon: Icon, text }: { icon: typeof Lock; text: string }) {
  return (
    <div className="m-auto flex flex-col items-center gap-2 text-center text-body-s text-ink-soft">
      <Icon aria-hidden className="size-5" />
      {text}
    </div>
  );
}

/** The location the page is on: a GMB's archive page, or the card open on the map. */
function useSubject(all: readonly Listed[]): Listed | null {
  const pathname = usePathname();
  const peeked = usePeeked();
  const route = /^\/review_archive\/gmbs\/(\d+)/.exec(pathname);
  const id = route ? Number(route[1]) : pathname === "/" ? peeked : null;
  return all.find((l) => l.gmb.id === id) ?? null;
}

/** A chat not started: what it can do here, and a few questions worth one press, just above the composer. */
function Empty({ subject, onAsk }: { subject: Listed | null; onAsk: (text: string) => void }) {
  const t = useT();
  const { tags } = useGmbTags();
  const name = subject ? (subject.gmb.name ?? subject.gmb.place_id) : null;
  return (
    <div className="mt-auto flex flex-col gap-3 pb-2">
      <div className="flex flex-col gap-2 px-2">
        <span className="flex size-10 items-center justify-center rounded-full bg-secondary text-ink">
          <Sparkles aria-hidden className="size-5" />
        </span>
        <h2 className="text-body-l font-semibold text-ink">{name ? t(assistant.empty.about, "What about {name}?", { name }) : t(assistant.empty, "What do you want to know?")}</h2>
        <p className="text-body-s text-ink-soft">
          {t(assistant.empty.hint.before, "Preview: answers are canned, nothing is sent. Name a location with ")}
          <Kbd>@</Kbd>
          {t(assistant.empty.hint.between, ", pick a command with ")}
          <Kbd>/</Kbd>
          {t(assistant.empty.hint.after, ".")}
        </p>
      </div>
      <div role="menu" aria-label={t(assistant.suggest, "Suggestions")} data-enter="stagger" className="flex flex-col">
        {suggestions(subject, tags).map((q) => (
          <button
            key={q}
            type="button"
            role="menuitem"
            onClick={() => onAsk(q)}
            className="flex h-8 items-center gap-2.5 rounded-md px-2 text-left text-body-s text-ink outline-none hover:bg-hover focus-visible:bg-hover"
          >
            <CornerDownRight aria-hidden className="size-4 shrink-0 text-ink-soft" />
            <span className="truncate">{q}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
