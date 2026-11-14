"use client";

import { Button, NavDot, cn, useKeymap as usePageKeymap } from "@evinvest/uikit";
import { Sparkles } from "lucide-react";
import { useEffect, useRef } from "react";

import { type ChatSummary, useChats, assistant } from "@/entities/assistant";
import { useT, keys as keysKeys } from "@/shared/i18n";
import { useQuery } from "@/shared/lib/query";
import { DESKTOP_QUERY, useMediaQuery } from "@/shared/lib/use-media-query";
import { ResizeEdge } from "@/shared/ui/resize-edge";

import { type Mode, WIDTH, notice, openChat, setWidth, show, useAssistant } from "../model/chat";
import { hideOnEscape, useKeymap } from "../model/keymap";
import { copyLink } from "./chat-actions";
import { ChatView } from "./chat-view";

/**
 * The assistant beside every signed-in page but its own: the top bar's `AssistantToggle` opens it to the sidebar
 * (resizable; the whole content row on a phone) or the floating window. It knows the location the page is on, and any
 * other the asker names with `@`. Mounted inside the content row, which it positions against.
 */
export function Assistant() {
  const t = useT();
  const s = useAssistant();
  const desktop = useMediaQuery(DESKTOP_QUERY);
  const shown = useQuery("assistant") as Mode | null;
  if (shown !== null && shown !== "sidebar" && shown !== "floating") throw new Error(`?assistant=${String(shown)}: not sidebar or floating`);
  // A link's display is the last shown, and whatever happens while open is seen.
  useEffect(() => {
    if (shown !== null) show(shown);
  }, [shown, s.unread]);
  const fold = () => show(null);
  useKeymap({
    toggle: () => (shown !== null ? fold() : show(s.mode)),
    newChat: () => openChat(null, shown ?? s.mode),
    copyLink: shown !== null && s.current !== null ? () => copyLink(s.current!, t) : null,
  });

  if (shown === null) return null;
  const view = <ChatView id={s.current} display={shown} onOpen={openChat} onHide={fold} />;
  if (!desktop)
    return (
      <section aria-label={t(assistant, "Assistant")} onKeyDown={(e) => hideOnEscape(e, fold)} className="absolute inset-0 z-30 flex flex-col bg-card">
        {view}
      </section>
    );
  if (shown === "floating")
    return (
      <section
        aria-label={t(assistant, "Assistant")}
        onKeyDown={(e) => hideOnEscape(e, fold)}
        className="absolute right-3.5 bottom-3.5 z-30 flex h-125 w-112 flex-col overflow-hidden rounded-t-3xl rounded-b-4xl bg-card shadow-overlay ring-1 ring-border"
      >
        {view}
      </section>
    );
  return (
    <Dock width={s.width} onHide={fold}>
      {view}
    </Dock>
  );
}

/** The top bar's press that opens or folds the assistant; it spins while an answer is written, and is dotted once something happened unseen. */
export function AssistantToggle() {
  const t = useT();
  const s = useAssistant();
  const shown = useQuery("assistant");
  const running = s.running !== null && (s.running.answer.state === "working" || s.running.answer.state === "writing");
  const button = useRef<HTMLButtonElement>(null);
  const keys = usePageKeymap().bound.get("assistant")?.sequence;
  // Folded from inside the assistant, focus lands here; not on a page loaded folded.
  const was = useRef(shown);
  useEffect(() => {
    if (was.current !== null && shown === null) button.current?.focus();
    was.current = shown;
  }, [shown]);
  useNoticeShared();
  return (
    <Button
      ref={button}
      type="button"
      variant="ghost"
      size="sm"
      icon
      className="relative"
      onClick={() => show(shown !== null ? null : s.mode)}
      aria-pressed={shown !== null}
      aria-label={t(keysKeys.assistant, "Assistant: show or hide")}
      title={keys === undefined ? t(keysKeys.assistant, "Assistant: show or hide") : `${t(keysKeys.assistant, "Assistant: show or hide")} · ${keys}`}
    >
      <Sparkles aria-hidden className={cn(running && "animate-pulse")} />
      {running && <span aria-hidden className="absolute inset-0 animate-spin rounded-full border-2 border-transparent border-t-primary [animation-duration:1.4s]" />}
      {s.unread && !running && <NavDot corner label={t(assistant.unread, "New answer")} />}
    </Button>
  );
}

/** While folded: a chat that changes (someone else's turn, a chat newly shared) is noticed. */
function useNoticeShared(): void {
  const chats = useChats();
  const last = useRef<Map<string, string> | null>(null);
  const data: ChatSummary[] | null = chats.status === "ok" ? chats.data : null;
  useEffect(() => {
    if (data === null) return;
    const now = new Map(data.map((c) => [c.id, c.updated_at]));
    const was = last.current;
    last.current = now;
    if (was !== null && data.some((c) => c.archived_at === null && was.get(c.id) !== c.updated_at)) notice();
  }, [data]);
}

/** The column beside the page, its left edge resizing it. */
function Dock({ width, onHide, children }: { width: number; onHide: () => void; children: React.ReactNode }) {
  const t = useT();
  return (
    <aside aria-label={t(assistant, "Assistant")} style={{ width }} onKeyDown={(e) => hideOnEscape(e, onHide)} className="relative flex shrink-0 flex-col border-l border-border bg-card">
      <ResizeEdge side="left" width={width} bounds={WIDTH} onWidth={setWidth} label={t(assistant.resize, "Resize the assistant")} />
      {children}
    </aside>
  );
}
