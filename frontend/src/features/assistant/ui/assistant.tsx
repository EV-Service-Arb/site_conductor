"use client";

import {
  ChatPanel,
  ConfirmDialog,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  IconButton,
  Input,
  Kbd,
  Popover,
  PopoverContent,
  PopoverTrigger,
  cn,
} from "@evinvest/uikit";
import { BookOpen, ChevronDown, ChevronsRight, CornerDownRight, Ellipsis, FileText, MapPin, PanelRight, Pencil, Search, Sparkles, SquarePen, Trash2, Upload } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import { type Listed, narrowed, setOnMap, useGmails, useGmbTags, useOnMap } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { DESKTOP_QUERY, useMediaQuery } from "@/shared/lib/use-media-query";

import { ask, asMentions, script, stop, suggestions } from "../model/canned";
import { type Chat, type Mention, type Source, WIDTH, openChat, removeChat, renameChat, setOpen, setWidth, useAssistant } from "../model/chat";
import { Composer, type ComposerHandle, type Draft } from "./composer";
import { AssistantTurnView, UserTurnView, When } from "./turns";

const SHORTCUT = "j";

/**
 * The assistant on every signed-in page: a launcher in the bottom-right corner, opening to a column beside the page
 * (resizable; the whole content row on a phone). It knows the location the page is on, and any other the asker names
 * with `@`. Mounted inside the content row, which it positions against.
 */
export function Assistant() {
  const t = useT();
  const s = useAssistant();
  const desktop = useMediaQuery(DESKTOP_QUERY);
  const launcher = useRef<HTMLButtonElement>(null);
  const running = s.chats.some((c) => {
    const last = c.turns.at(-1);
    return last?.role === "assistant" && (last.state === "working" || last.state === "writing");
  });

  const fold = () => {
    setOpen(false);
    requestAnimationFrame(() => launcher.current?.focus());
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() !== SHORTCUT || !(e.metaKey || e.ctrlKey) || e.altKey || e.shiftKey) return;
      e.preventDefault();
      if (s.open) fold();
      else setOpen(true);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [s.open]);

  if (!s.open)
    return (
      <button
        ref={launcher}
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

  const view = <ChatView onFold={fold} />;
  if (desktop) return <Dock width={s.width}>{view}</Dock>;
  return (
    <section
      aria-label={t("assistant")}
      onKeyDown={(e) => {
        // A menu or popover is portalled out: its Escape closes it, not the assistant.
        if (e.key === "Escape" && !e.defaultPrevented && e.currentTarget.contains(e.target as Node)) fold();
      }}
      className="absolute inset-0 z-30 flex flex-col bg-card"
    >
      {view}
    </section>
  );
}

/** The column beside the page, its left edge dragged (or arrowed) to resize. */
function Dock({ width, children }: { width: number; children: React.ReactNode }) {
  const t = useT();
  const [dragging, setDragging] = useState(false);
  return (
    <aside aria-label={t("assistant")} style={{ width }} className="relative flex shrink-0 flex-col border-l border-border bg-card">
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

/** Header, conversation and composer; the same in the window and the column. */
function ChatView({ onFold }: { onFold: () => void }) {
  const t = useT();
  const s = useAssistant();
  const chat = s.chats.find((c) => c.id === s.current) ?? null;
  const gmails = useGmails();
  const { tags } = useGmbTags();
  const all = gmails.status === "ok" ? narrowed(gmails.data, null, "") : [];
  const subject = useSubject(all);
  const composer = useRef<ComposerHandle>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [dropping, setDropping] = useState(0);
  const [deleting, setDeleting] = useState<Chat | null>(null);
  const last = chat?.turns.at(-1);
  const streaming = last?.role === "assistant" && (last.state === "working" || last.state === "writing");

  const resolve = (mentions: readonly Mention[]): Listed[] => mentions.flatMap((m) => all.filter((l) => l.gmb.id === m.id));
  const send = (draft: Draft) => {
    const named = [...(subject && !draft.subjectOff ? asMentions([subject]) : []), ...draft.mentions].filter((m, i, xs) => xs.findIndex((x) => x.id === m.id) === i);
    ask(chat?.id ?? null, chat?.turns.length ?? 0, { text: draft.text, mentions: named, files: draft.files }, script(draft.text, resolve(named), all, tags, t), s.approval);
  };
  const again = (at: number, text: string) => {
    const asked = chat!.turns[at];
    if (asked?.role !== "user") throw new Error("a question is re-asked from its own turn");
    setEditing(null);
    composer.current?.focus();
    ask(chat!.id, at, { text, mentions: asked.mentions, files: asked.files }, script(text, resolve(asked.mentions), all, tags, t), s.approval);
  };

  return (
    <div
      className="relative flex min-h-0 flex-1 flex-col"
      onDragEnter={(e) => e.dataTransfer.types.includes("Files") && setDropping((n) => n + 1)}
      onDragLeave={(e) => e.dataTransfer.types.includes("Files") && setDropping((n) => n - 1)}
      onDragOver={(e) => e.dataTransfer.types.includes("Files") && e.preventDefault()}
      onDrop={(e) => {
        if (!e.dataTransfer.types.includes("Files")) return;
        e.preventDefault();
        setDropping(0);
        composer.current?.attach([...e.dataTransfer.files]);
      }}
    >
      <Header chat={chat} onFold={onFold} onDelete={setDeleting} onRenamed={() => composer.current?.focus()} />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => {
          if (!open) setDeleting(null);
        }}
        tone="error"
        title={t("assistant.delete.title")}
        description={deleting?.title ?? t("assistant.newChat")}
        confirmLabel={t("assistant.delete")}
        labels={{ cancel: t("assistant.edit.cancel") }}
        onConfirm={() => removeChat(deleting!.id)}
      />
      <ChatPanel
        streaming={streaming}
        labels={{ jump: t("assistant.jump"), log: t("assistant.log") }}
        empty={<Empty subject={subject} onAsk={(text) => send({ text, mentions: [], files: [], subjectOff: false })} />}
        composer={
          <Composer
              key={chat?.id ?? ""}
              handle={composer}
              chat={chat?.id ?? null}
              subject={subject}
              streaming={streaming}
              approval={s.approval}
              onSend={send}
              onStop={stop}
              onEditLast={() => {
                const asked = chat?.turns.findLast((x) => x.role === "user");
                if (asked) setEditing(asked.id);
              }}
            />
        }
      >
        {chat?.turns.map((turn, i) =>
          turn.role === "user" ? (
            <UserTurnView key={turn.id} turn={turn} editing={editing === turn.id} onEditing={(on) => {
                setEditing(on ? turn.id : null);
                if (!on) composer.current?.focus();
              }} onEdit={(text) => again(i, text)} />
          ) : (
            <AssistantTurnView key={turn.id} chat={chat.id} turn={turn} last={i === chat.turns.length - 1} onRetry={() => again(i - 1, (chat.turns[i - 1] as { text: string }).text)} />
          ),
        )}
      </ChatPanel>
      {dropping > 0 && (
        <div className="pointer-events-none absolute inset-2 z-20 flex flex-col items-center justify-center gap-2 rounded-panel border-2 border-dashed border-ring bg-card/90 text-body text-ink">
          <Upload aria-hidden className="size-6 text-ink-soft" />
          {t("assistant.drop")}
        </div>
      )}
    </div>
  );
}

/** The location the page is on: a GMB's archive page, or the card open on the map. */
function useSubject(all: readonly Listed[]): Listed | null {
  const pathname = usePathname();
  const { peeked } = useOnMap();
  const route = /^\/review_archive\/gmbs\/(\d+)/.exec(pathname);
  const id = route ? Number(route[1]) : pathname === "/" ? peeked : null;
  return all.find((l) => l.gmb.id === id) ?? null;
}

function Header({ chat, onFold, onDelete, onRenamed }: { chat: Chat | null; onFold: () => void; onDelete: (chat: Chat) => void; onRenamed: () => void }) {
  const t = useT();
  const [renaming, setRenaming] = useState(false);
  const title = chat?.title ?? t("assistant.newChat");
  return (
    <header className="flex h-11 shrink-0 items-center gap-0.5 px-2">
      {renaming && chat ? (
        <Input
          autoFocus
          defaultValue={chat.title ?? ""}
          aria-label={t("assistant.rename")}
          className="h-7 min-w-0 flex-1"
          onFocus={(e) => e.currentTarget.select()}
          onBlur={(e) => {
            renameChat(chat.id, e.currentTarget.value);
            setRenaming(false);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              e.currentTarget.value = chat.title ?? "";
            }
            if (e.key === "Enter" || e.key === "Escape") {
              e.currentTarget.blur();
              onRenamed();
            }
          }}
        />
      ) : (
        <History title={title} onDelete={onDelete} />
      )}
      <span className="flex-1" />
      <IconButton size="sm" aria-label={t("assistant.new")} title={t("assistant.new")} disabled={chat === null} onClick={() => openChat(null)}>
        <SquarePen />
      </IconButton>
      <Context chat={chat} />
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <IconButton size="sm" aria-label={t("assistant.more")} title={t("assistant.more")} disabled={chat === null}>
            <Ellipsis />
          </IconButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onClick={() => setRenaming(true)}>
            <Pencil aria-hidden />
            {t("assistant.rename")}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onClick={() => onDelete(chat!)}>
            <Trash2 aria-hidden />
            {t("assistant.delete")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <IconButton size="sm" aria-label={t("assistant.fold")} title={`${t("assistant.fold")} · Ctrl+J`} onClick={onFold}>
        <ChevronsRight />
      </IconButton>
    </header>
  );
}

/** What the chat has touched: the locations named in it and what its answers read. */
function Context({ chat }: { chat: Chat | null }) {
  const t = useT();
  const locations = new Map<number, Mention>();
  const read = new Map<string, Source>();
  for (const turn of chat?.turns ?? []) {
    if (turn.role === "user") for (const m of turn.mentions) locations.set(m.id, m);
    else {
      for (const [, label, id] of turn.text.matchAll(/@\[([^\]]*)\]\(location:(\d+)\)/g)) locations.set(Number(id), { kind: "location", id: Number(id), label: label! });
      for (const source of turn.sources) read.set(source.label, source);
    }
  }
  const none = locations.size === 0 && read.size === 0;
  const row = "flex h-8 items-center gap-2 rounded-md px-2 text-body-s text-ink outline-none hover:bg-hover focus-visible:bg-hover";
  return (
    <Popover>
      <PopoverTrigger asChild>
        <IconButton size="sm" aria-label={t("assistant.context")} title={t("assistant.context")} disabled={none} className="aria-expanded:bg-hover">
          <PanelRight />
        </IconButton>
      </PopoverTrigger>
      <PopoverContent align="end" className="flex w-80 flex-col gap-2 rounded-xl p-1.5">
        {locations.size > 0 && (
          <section className="flex flex-col">
            <h3 className="px-2 py-1 text-caption text-ink-soft">{t("assistant.context.locations")}</h3>
            {[...locations.values()].map((m) => (
              <Link key={m.id} href="/" onClick={() => setOnMap({ peeked: m.id })} className={row}>
                <MapPin aria-hidden className="size-4 shrink-0 text-ink-soft" />
                <span className="truncate">{m.label}</span>
              </Link>
            ))}
          </section>
        )}
        {read.size > 0 && (
          <section className="flex flex-col">
            <h3 className="px-2 py-1 text-caption text-ink-soft">{t("assistant.context.read")}</h3>
            {[...read.values()].map((source) =>
              source.href === null ? (
                <span key={source.label} className={cn(row, "hover:bg-transparent")}>
                  <BookOpen aria-hidden className="size-4 shrink-0 text-ink-soft" />
                  <span className="truncate">{source.label}</span>
                </span>
              ) : (
                <Link key={source.label} href={source.href} className={row}>
                  <FileText aria-hidden className="size-4 shrink-0 text-ink-soft" />
                  <span className="truncate">{source.label}</span>
                </Link>
              ),
            )}
          </section>
        )}
      </PopoverContent>
    </Popover>
  );
}

/** The chat's title, opening every chat this browser keeps: searchable, newest first. */
function History({ title, onDelete }: { title: string; onDelete: (chat: Chat) => void }) {
  const t = useT();
  const s = useAssistant();
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const [active, setActive] = useState(0);
  const needle = filter.trim().toLowerCase();
  const found = s.chats.filter((c) => needle === "" || (c.title ?? "").toLowerCase().includes(needle) || c.turns.some((x) => x.role === "user" && x.text.toLowerCase().includes(needle)));
  const today = new Date().toDateString();
  const groups = [
    { label: t("assistant.history.today"), chats: found.filter((c) => new Date(c.updated).toDateString() === today) },
    { label: t("assistant.history.earlier"), chats: found.filter((c) => new Date(c.updated).toDateString() !== today) },
  ].filter((g) => g.chats.length > 0);
  const flat = groups.flatMap((g) => g.chats);
  const pick = (c: Chat) => {
    openChat(c.id);
    setOpen(false);
    setFilter("");
  };
  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        setActive(0);
      }}
    >
      <PopoverTrigger className="flex h-7 min-w-0 items-center gap-1.5 rounded-md pr-1.5 pl-1 text-left text-body text-ink outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring aria-expanded:bg-hover">
        <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-hover">
          <Sparkles aria-hidden className="size-3 text-ink-mid" />
        </span>
        <span className="truncate">{title}</span>
        <ChevronDown aria-hidden className="size-3.5 shrink-0 text-ink-soft" />
      </PopoverTrigger>
      <PopoverContent align="start" className="flex w-80 flex-col gap-1 rounded-xl p-1.5">
        <div className="relative">
          <Search aria-hidden className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-ink-soft" />
          <Input
            autoFocus
            value={filter}
            placeholder={t("assistant.history.search")}
            aria-label={t("assistant.history.search")}
            className="h-8 pl-7"
            onChange={(e) => {
              setFilter(e.target.value);
              setActive(0);
            }}
            onKeyDown={(e) => {
              const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
              if (step !== 0 && flat.length > 0) {
                e.preventDefault();
                setActive((a) => (a + step + flat.length) % flat.length);
              }
              if (e.key === "Enter" && flat[active]) pick(flat[active]);
            }}
          />
        </div>
        <div role="listbox" aria-label={t("assistant.history")} className="flex max-h-80 flex-col overflow-y-auto">
          {flat.length === 0 && <p className="px-2 py-3 text-center text-body-s text-ink-soft">{t(s.chats.length === 0 ? "assistant.history.none" : "assistant.history.nothing")}</p>}
          {groups.map((g) => (
            <div key={g.label} className="flex flex-col">
              <div className="px-2 pt-2 pb-1 text-caption text-ink-soft">{g.label}</div>
              {g.chats.map((c) => {
                const i = flat.indexOf(c);
                return (
                  <div
                    key={c.id}
                    role="option"
                    aria-selected={i === active}
                    aria-current={c.id === s.current ? "true" : undefined}
                    onMouseMove={() => setActive(i)}
                    onClick={() => pick(c)}
                    className={cn("group/row flex h-8 cursor-pointer items-center gap-2 rounded-md pr-1 pl-2 text-body-s text-ink", i === active && "bg-hover", c.id === s.current && "font-medium")}
                  >
                    <span className="min-w-0 flex-1 truncate">{c.title ?? t("assistant.newChat")}</span>
                    <span className="text-caption text-ink-faint group-hover/row:hidden">
                      <When at={c.updated} />
                    </span>
                    <IconButton
                      size="sm"
                      aria-label={t("assistant.delete")}
                      title={t("assistant.delete")}
                      className="hidden size-6 group-hover/row:inline-flex"
                      onClick={(e) => {
                        e.stopPropagation();
                        setOpen(false);
                        onDelete(c);
                      }}
                    >
                      <Trash2 />
                    </IconButton>
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
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
        <h2 className="text-body-l font-semibold text-ink">{name ? t("assistant.empty.about", { name }) : t("assistant.empty")}</h2>
        <p className="text-body-s text-ink-soft">
          {t("assistant.empty.hint.before")}
          <Kbd>@</Kbd>
          {t("assistant.empty.hint.between")}
          <Kbd>/</Kbd>
          {t("assistant.empty.hint.after")}
        </p>
      </div>
      <div role="menu" aria-label={t("assistant.suggest")} data-enter="stagger" className="flex flex-col">
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
