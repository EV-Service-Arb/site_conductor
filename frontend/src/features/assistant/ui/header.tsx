"use client";

import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger, IconButton, Input, Kbd, KbdGroup, Popover, PopoverContent, PopoverTrigger, cn } from "@evinvest/uikit";
import { BookOpen, ChevronDown, ChevronsRight, Ellipsis, FileText, MapPin, MessageCirclePlus, Minus, PanelRight, Pin, PinOff, Share as ShareIcon, Sparkles } from "lucide-react";
import Link from "next/link";
import { useState } from "react";

import { type ChatSummary, type Mention, type Source, type Turn, pinChat, renameChat } from "@/entities/assistant";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

import { ChatActions, useControls } from "./chat-actions";
import { History } from "./history";
import { Share } from "./share";

/**
 * The open chat's title (renamed in place by whoever can interact), its history, and the chat's controls. `chat` is
 * `null` for one not started.
 */
export function Header({
  chat,
  turns,
  onOpen,
  onHide,
  onRenamed,
  context,
  onContext,
}: {
  chat: ChatSummary | null;
  turns: readonly Turn[];
  onOpen: (id: string | null) => void;
  /** The sidebar's or the floating window's; none on the full screen page. */
  onHide: (() => void) | null;
  onRenamed: () => void;
  /** The full screen page's context card, shown; `null` where context is a popover. */
  context: boolean | null;
  onContext: () => void;
}) {
  const t = useT();
  const { display, onChanged } = useControls();
  const [renaming, setRenaming] = useState(false);
  const full = display === "full";
  const interacts = chat !== null && chat.role !== "viewer";
  const title = chat?.title ?? t("assistant.newChat");
  const rename = (value: string) => {
    setRenaming(false);
    onRenamed();
    if (chat !== null && value.trim() !== (chat.title ?? "")) void renameChat(chat.id, value).then(onChanged, (e: unknown) => notifyFailure(e, t));
  };
  const history = (
    <History current={chat?.id ?? null} onOpen={onOpen}>
      <IconButton size="sm" aria-label={t("assistant.history")} tooltip={t("assistant.history")} className="size-6 aria-expanded:bg-hover">
        <ChevronDown />
      </IconButton>
    </History>
  );
  return (
    <header className={cn("flex h-11 shrink-0 items-center gap-0.5 px-2", full && "h-12 px-3")}>
      {!full && (
        <span className="mr-1 flex size-6 shrink-0 items-center justify-center rounded-full bg-hover">
          <Sparkles aria-hidden className="size-3.5 text-ink-mid" />
        </span>
      )}
      {full && <span className="mr-1.5 text-body-s text-ink-soft">{t("assistant")} /</span>}
      {renaming && chat !== null ? (
        <Input
          autoFocus
          defaultValue={chat.title ?? ""}
          aria-label={t("assistant.rename")}
          className="h-7 min-w-0 flex-1"
          onFocus={(e) => e.currentTarget.select()}
          onBlur={(e) => rename(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key !== "Enter" && e.key !== "Escape") return;
            // Kept from the composer, which takes focus as the field goes.
            e.preventDefault();
            if (e.key === "Escape") e.currentTarget.value = chat.title ?? "";
            e.currentTarget.blur();
          }}
        />
      ) : interacts ? (
        <button
          type="button"
          onClick={() => setRenaming(true)}
          aria-label={t("assistant.renameTitle", { title })}
          className="h-7 min-w-0 truncate rounded-md px-1.5 text-left text-body-s font-medium text-ink outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring"
        >
          {title}
        </button>
      ) : (
        <span className="min-w-0 truncate px-1.5 text-body-s font-medium text-ink">{title}</span>
      )}
      {history}
      <span className="flex-1" />
      <IconButton
        size="sm"
        aria-label={t("assistant.new")}
        tooltip={
          <span className="flex items-center gap-2">
            {t("assistant.new")}
            <KbdGroup>
              <Kbd>Ctrl</Kbd>
              <Kbd>⇧</Kbd>
              <Kbd>;</Kbd>
            </KbdGroup>
          </span>
        }
        disabled={chat === null}
        onClick={() => onOpen(null)}
      >
        <MessageCirclePlus />
      </IconButton>
      {chat !== null ? (
        <Share chat={chat}>
          <IconButton size="sm" aria-label={t("assistant.share")} tooltip={t("assistant.share")} className="aria-expanded:bg-hover">
            <ShareIcon />
          </IconButton>
        </Share>
      ) : (
        <IconButton size="sm" aria-label={t("assistant.share")} disabled>
          <ShareIcon />
        </IconButton>
      )}
      {context === null ? (
        <Context turns={turns} />
      ) : (
        <IconButton size="sm" aria-label={t("assistant.context")} tooltip={t("assistant.context")} pressed={context} onClick={onContext}>
          <PanelRight />
        </IconButton>
      )}
      {full && interacts && (
        <IconButton
          size="sm"
          aria-label={t(chat.pinned_at === null ? "assistant.pin" : "assistant.unpin")}
          tooltip={t(chat.pinned_at === null ? "assistant.pin" : "assistant.unpin")}
          pressed={chat.pinned_at !== null}
          onClick={() => void pinChat(chat.id, chat.pinned_at === null).then(onChanged, (e: unknown) => notifyFailure(e, t))}
        >
          {chat.pinned_at === null ? <Pin /> : <PinOff />}
        </IconButton>
      )}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <IconButton size="sm" aria-label={t("assistant.more")} tooltip={t("assistant.more")} disabled={chat === null}>
            <Ellipsis />
          </IconButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-52">
          {chat !== null && <ChatActions chat={chat} onRename={() => setRenaming(true)} pinOutside={full} />}
        </DropdownMenuContent>
      </DropdownMenu>
      {onHide !== null && (
        <IconButton
          size="sm"
          aria-label={t(display === "floating" ? "assistant.minimize" : "assistant.fold")}
          tooltip={
            <span className="flex items-center gap-2">
              {t(display === "floating" ? "assistant.minimize" : "assistant.fold")}
              <KbdGroup>
                <Kbd>Ctrl</Kbd>
                <Kbd>J</Kbd>
              </KbdGroup>
            </span>
          }
          onClick={onHide}
        >
          {display === "floating" ? <Minus /> : <ChevronsRight />}
        </IconButton>
      )}
    </header>
  );
}

/** The locations named in a chat and what its answers read. */
function touched(turns: readonly Turn[]) {
  const locations = new Map<number, Mention>();
  const read = new Map<string, Source>();
  for (const turn of turns) {
    if (turn.role === "user") for (const m of turn.mentions) locations.set(m.id, m);
    else {
      for (const [, label, id] of turn.text.matchAll(/@\[([^\]]*)\]\(location:(\d+)\)/g)) locations.set(Number(id), { kind: "location", id: Number(id), label: label! });
      for (const source of turn.sources) read.set(source.label, source);
    }
  }
  return { locations: [...locations.values()], read: [...read.values()] };
}

function Context({ turns }: { turns: readonly Turn[] }) {
  const t = useT();
  const { locations, read } = touched(turns);
  return (
    <Popover>
      <PopoverTrigger asChild>
        <IconButton size="sm" aria-label={t("assistant.context")} tooltip={t("assistant.context")} disabled={locations.length === 0 && read.length === 0} className="aria-expanded:bg-hover">
          <PanelRight />
        </IconButton>
      </PopoverTrigger>
      <PopoverContent align="end" className="flex w-80 flex-col gap-2 rounded-xl p-1.5">
        <Touched turns={turns} />
      </PopoverContent>
    </Popover>
  );
}

/** The full screen page's context, a card beside the conversation. */
export function ContextCard({ turns }: { turns: readonly Turn[] }) {
  const t = useT();
  const { locations, read } = touched(turns);
  return (
    <aside aria-label={t("assistant.context")} className="flex w-64 shrink-0 flex-col gap-2 self-start rounded-xl border border-border-subtle p-1.5">
      {locations.length === 0 && read.length === 0 ? <p className="px-2 py-1.5 text-body-s text-ink-soft">{t("assistant.context.none")}</p> : <Touched turns={turns} />}
    </aside>
  );
}

function Touched({ turns }: { turns: readonly Turn[] }) {
  const t = useT();
  const { locations, read } = touched(turns);
  const row = "flex h-8 items-center gap-2 rounded-md px-2 text-body-s text-ink outline-none hover:bg-hover focus-visible:bg-hover";
  return (
    <>
      {locations.length > 0 && (
        <section className="flex flex-col">
          <h3 className="px-2 py-1 text-caption text-ink-soft">{t("assistant.context.locations")}</h3>
          {locations.map((m) => (
            <Link key={m.id} href={`/?gmb=${m.id}`} className={row}>
              <MapPin aria-hidden className="size-4 shrink-0 text-ink-soft" />
              <span className="truncate">{m.label}</span>
            </Link>
          ))}
        </section>
      )}
      {read.length > 0 && (
        <section className="flex flex-col">
          <h3 className="px-2 py-1 text-caption text-ink-soft">{t("assistant.context.read")}</h3>
          {read.map((source) =>
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
    </>
  );
}
