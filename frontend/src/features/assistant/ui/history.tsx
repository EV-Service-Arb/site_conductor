"use client";

import { Button, DropdownMenu, DropdownMenuContent, DropdownMenuTrigger, IconButton, Input, Popover, PopoverContent, PopoverTrigger, Spinner, cn } from "@evinvest/uikit";
import { Archive, ArchiveRestore, ArrowLeft, Ellipsis, MessageCircle, Search, Users } from "lucide-react";
import { type ReactNode, useRef, useState } from "react";

import { type ChatSummary, archiveChat, renameChat, useChats } from "@/entities/assistant";
import { type MessageKey, useLocale, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

import { ChatActions, ControlsContext, useControls } from "./chat-actions";
import { age } from "./time";

const DAY = 24 * 60 * 60 * 1000;

function groupsOf(chats: ChatSummary[]): { label: MessageKey; chats: ChatSummary[] }[] {
  const midnight = new Date().setHours(0, 0, 0, 0);
  const since = (c: ChatSummary) => Date.parse(c.updated_at);
  const unpinned = chats.filter((c) => c.pinned_at === null);
  return [
    { label: "assistant.history.pinned" as const, chats: chats.filter((c) => c.pinned_at !== null).sort((a, b) => b.pinned_at!.localeCompare(a.pinned_at!)) },
    { label: "assistant.history.today" as const, chats: unpinned.filter((c) => since(c) >= midnight) },
    { label: "assistant.history.week" as const, chats: unpinned.filter((c) => since(c) < midnight && since(c) >= midnight - 6 * DAY) },
    { label: "assistant.history.month" as const, chats: unpinned.filter((c) => since(c) < midnight - 6 * DAY && since(c) >= midnight - 29 * DAY) },
    { label: "assistant.history.older" as const, chats: unpinned.filter((c) => since(c) < midnight - 29 * DAY) },
  ].filter((g) => g.chats.length > 0);
}

/** Every chat of the caller's, opened from `trigger`: pinned first, then by when it last changed; the archived behind its own view. */
export function History({ current, onOpen, children }: { current: string | null; onOpen: (id: string) => void; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align="start" className="flex w-76 flex-col rounded-xl p-0">
        <Chats
          current={current}
          onOpen={(id) => {
            setOpen(false);
            onOpen(id);
          }}
        />
      </PopoverContent>
    </Popover>
  );
}

function Chats({ current, onOpen }: { current: string | null; onOpen: (id: string) => void }) {
  const t = useT();
  const controls = useControls();
  const list = useChats();
  const [archived, setArchived] = useState(false);
  const [filter, setFilter] = useState("");
  const [active, setActive] = useState(0);
  const [renaming, setRenaming] = useState<string | null>(null);
  const search = useRef<HTMLInputElement>(null);
  const needle = filter.trim().toLowerCase();
  const all = list.status === "ok" ? list.data : [];
  const found = all.filter((c) => (c.archived_at !== null) === archived && (needle === "" || (c.title ?? t("assistant.newChat")).toLowerCase().includes(needle)));
  const groups = archived ? [{ label: "assistant.history.archived" as const, chats: found }] : groupsOf(found);
  const flat = groups.flatMap((g) => g.chats);
  // A row's menu or a whole view going away would leave focus nowhere: it comes back to the search.
  const refocus = () => requestAnimationFrame(() => search.current?.focus());
  const changed = () => {
    controls.onChanged();
    list.reload();
    refocus();
  };
  const view = (to: boolean) => {
    setArchived(to);
    setActive(0);
    refocus();
  };
  const rename = (c: ChatSummary, title: string) => {
    setRenaming(null);
    if (title.trim() !== (c.title ?? "")) void renameChat(c.id, title).then(changed, (e: unknown) => notifyFailure(e, t));
  };

  return (
    <ControlsContext.Provider value={{ ...controls, onChanged: changed }}>
      <div
        className="contents"
        onKeyDownCapture={(e) => {
          // Typing anywhere in the list searches it.
          if (e.key.length !== 1 || e.ctrlKey || e.metaKey || e.altKey || e.target instanceof HTMLInputElement) return;
          e.preventDefault();
          setFilter((f) => f + e.key);
          setActive(0);
          search.current?.focus();
        }}
      >
      {archived ? (
        <div className="flex items-center gap-1 px-1.5 pt-1.5">
          <IconButton size="sm" aria-label={t("assistant.history.back")} tooltip={t("assistant.history.back")} onClick={() => view(false)}>
            <ArrowLeft />
          </IconButton>
          <h3 className="flex-1 text-body-s font-medium text-ink">{t("assistant.history.archived")}</h3>
          <Button size="sm" variant="ghost" disabled={!all.some((c) => c.archived_at !== null)} onClick={() => controls.onConfirm({ kind: "clear" })}>
            {t("assistant.history.clear")}
          </Button>
        </div>
      ) : null}
      <div className="relative mx-1.5 mt-1.5">
        <Search aria-hidden className="pointer-events-none absolute top-1/2 right-2.5 size-3.5 -translate-y-1/2 text-ink-soft" />
        <Input
          ref={search}
          autoFocus
          value={filter}
          placeholder={t("assistant.history.search")}
          aria-label={t("assistant.history.search")}
          className="h-8 border-0 bg-transparent pr-7 shadow-none focus-visible:ring-0"
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
            if (e.key === "Enter" && flat[active]) onOpen(flat[active].id);
          }}
        />
      </div>
      <div role="listbox" aria-label={t("assistant.history")} className="flex max-h-96 flex-col overflow-y-auto p-1">
        {list.status === "loading" && <Spinner className="m-3 size-4 self-center" />}
        {list.status === "error" && <p className="px-2 py-3 text-center text-body-s text-negative">{t("assistant.history.failed")}</p>}
        {list.status === "ok" && flat.length === 0 && (
          <p className="px-2 py-3 text-center text-body-s text-ink-soft">
            {t(needle !== "" ? "assistant.history.nothing" : archived ? "assistant.history.noArchived" : "assistant.history.none")}
          </p>
        )}
        {groups.map((g) => (
          <div key={g.label} className="flex flex-col">
            {!archived && <div className="px-2 pt-2 pb-1 text-caption font-medium text-ink-soft">{t(g.label)}</div>}
            {g.chats.map((c) => {
              const i = flat.indexOf(c);
              return renaming === c.id ? (
                <Input
                  key={c.id}
                  autoFocus
                  defaultValue={c.title ?? ""}
                  aria-label={t("assistant.rename")}
                  className="h-8"
                  onFocus={(e) => e.currentTarget.select()}
                  onBlur={(e) => rename(c, e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key !== "Enter" && e.key !== "Escape") return;
                    e.preventDefault();
                    if (e.key === "Enter") rename(c, e.currentTarget.value);
                    else setRenaming(null);
                  }}
                />
              ) : (
                <div
                  key={c.id}
                  role="option"
                  aria-selected={i === active}
                  aria-current={c.id === current ? "true" : undefined}
                  onMouseMove={() => setActive(i)}
                  onClick={() => onOpen(c.id)}
                  className={cn("group/row flex h-8 cursor-pointer items-center gap-2 rounded-md pr-1 pl-2 text-body-s text-ink", i === active && "bg-hover", c.id === current && "font-medium")}
                >
                  {c.shared ? <Users aria-hidden className="size-4 shrink-0 text-ink-soft" /> : <MessageCircle aria-hidden className="size-4 shrink-0 text-ink-soft" />}
                  <span className="min-w-0 flex-1 truncate">{c.title ?? t("assistant.newChat")}</span>
                  {archived ? (
                    <IconButton
                      size="sm"
                      aria-label={t("assistant.unarchive")}
                      tooltip={t("assistant.unarchive")}
                      className="size-6"
                      onClick={(e) => {
                        e.stopPropagation();
                        void archiveChat(c.id, false).then(changed, (err: unknown) => notifyFailure(err, t));
                      }}
                    >
                      <ArchiveRestore />
                    </IconButton>
                  ) : (
                    <RowMenu chat={c} onRename={() => setRenaming(c.id)} />
                  )}
                </div>
              );
            })}
          </div>
        ))}
      </div>
      {!archived && (
        <div className="border-t border-border-subtle p-1">
          <button
            type="button"
            onClick={() => view(true)}
            className="flex h-7 items-center gap-2 rounded-md px-2 text-body-s text-ink-mid outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring"
          >
            <Archive aria-hidden className="size-3.5" />
            {t("assistant.history.archived")}
          </button>
        </div>
      )}
      </div>
    </ControlsContext.Provider>
  );
}

/** A row's age, giving way on hover to its ⋯. */
function RowMenu({ chat, onRename }: { chat: ChatSummary; onRename: () => void }) {
  const t = useT();
  const locale = useLocale();
  const [open, setOpen] = useState(false);
  return (
    <>
      <span className={cn("text-caption text-ink-faint group-hover/row:hidden", open && "hidden")}>{age(chat.updated_at, locale)}</span>
      <DropdownMenu open={open} onOpenChange={setOpen}>
        <DropdownMenuTrigger asChild>
          <IconButton size="sm" aria-label={t("assistant.more")} className={cn("hidden size-6 group-hover/row:inline-flex", open && "inline-flex")} onClick={(e) => e.stopPropagation()}>
            <Ellipsis />
          </IconButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-52" onClick={(e) => e.stopPropagation()}>
          <ChatActions chat={chat} onRename={onRename} />
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  );
}
