"use client";

import { cn } from "@evinvest/uikit";
import { Sparkles } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { type ChatSummary, useChats } from "@/entities/assistant";
import { useT } from "@/shared/i18n";
import { DESKTOP_QUERY, useMediaQuery } from "@/shared/lib/use-media-query";

import { WIDTH, notice, openChat, setOpen, setWidth, useAssistant } from "../model/chat";
import { hideOnEscape, useKeymap } from "../model/keymap";
import { copyLink } from "./chat-actions";
import { ChatView } from "./chat-view";

/**
 * The assistant beside every signed-in page but its own: a launcher in the bottom-right corner, opening to the sidebar
 * (resizable; the whole content row on a phone) or the floating window. It knows the location the page is on, and any
 * other the asker names with `@`. Mounted inside the content row, which it positions against.
 */
export function Assistant() {
  const t = useT();
  const s = useAssistant();
  const desktop = useMediaQuery(DESKTOP_QUERY);
  // Folded by a press or a key, the launcher takes focus; not on a page loaded folded.
  const [folded, setFolded] = useState(false);

  const fold = () => {
    setFolded(true);
    setOpen(false);
  };
  useKeymap({
    toggle: () => (s.open ? fold() : setOpen(true)),
    newChat: () => openChat(null, s.mode),
    copyLink: s.open && s.current !== null ? () => copyLink(s.current!, t) : null,
  });

  if (!s.open) return <Launcher focus={folded} />;
  const view = <ChatView id={s.current} display={s.mode} onOpen={openChat} onHide={fold} />;
  if (!desktop)
    return (
      <section aria-label={t("assistant")} onKeyDown={(e) => hideOnEscape(e, fold)} className="absolute inset-0 z-30 flex flex-col bg-card">
        {view}
      </section>
    );
  if (s.mode === "floating")
    return (
      <section
        aria-label={t("assistant")}
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

/** Folded: a press opens the assistant; it spins while an answer is written, and is dotted once something happened unseen. */
function Launcher({ focus }: { focus: boolean }) {
  const t = useT();
  const s = useAssistant();
  const running = s.running !== null && (s.running.answer.state === "working" || s.running.answer.state === "writing");
  useNoticeShared();
  return (
    <button
      autoFocus={focus}
      type="button"
      onClick={() => setOpen(true)}
      aria-label={t("assistant.open")}
      title={`${t("assistant.open")} · Ctrl+J`}
      className="absolute right-4 bottom-4 z-30 flex size-10 items-center justify-center rounded-full border border-border bg-secondary text-ink outline-none transition-[translate,background-color] hover:-translate-y-0.5 hover:bg-popover focus-visible:ring-2 focus-visible:ring-ring"
    >
      <Sparkles aria-hidden className={cn("size-5", running && "animate-pulse")} />
      {running && <span aria-hidden className="absolute inset-0 animate-spin rounded-full border-2 border-transparent border-t-primary [animation-duration:1.4s]" />}
      {s.unread && !running && <span aria-label={t("assistant.unread")} className="absolute top-0 right-0 size-3 rounded-full border-2 border-card bg-primary" />}
    </button>
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

/** The column beside the page, its left edge dragged (or arrowed) to resize. */
function Dock({ width, onHide, children }: { width: number; onHide: () => void; children: React.ReactNode }) {
  const t = useT();
  const [dragging, setDragging] = useState(false);
  return (
    <aside aria-label={t("assistant")} style={{ width }} onKeyDown={(e) => hideOnEscape(e, onHide)} className="relative flex shrink-0 flex-col border-l border-border bg-card">
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label={t("assistant.resize")}
        aria-valuemin={WIDTH.min}
        aria-valuemax={WIDTH.max}
        aria-valuenow={width}
        tabIndex={0}
        onKeyDown={(e) => {
          const step = e.key === "ArrowLeft" ? 24 : e.key === "ArrowRight" ? -24 : 0;
          if (step === 0) return;
          e.preventDefault();
          setWidth(width + step);
        }}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          setDragging(true);
        }}
        onPointerMove={(e) => dragging && setWidth(window.innerWidth - e.clientX)}
        onPointerUp={() => setDragging(false)}
        onDoubleClick={() => setWidth(WIDTH.initial)}
        className={cn(
          "absolute inset-y-0 -left-1 z-10 w-2 cursor-col-resize outline-none after:absolute after:inset-y-0 after:left-0.75 after:w-0.5 after:transition-colors hover:after:bg-border-strong focus-visible:after:bg-ring",
          dragging && "after:bg-ring",
        )}
      />
      {dragging && <div className="fixed inset-0 z-50 cursor-col-resize" />}
      {children}
    </aside>
  );
}
