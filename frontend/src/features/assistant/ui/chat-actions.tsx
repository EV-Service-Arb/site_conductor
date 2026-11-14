"use client";

import { DropdownMenuItem, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuShortcut, toast } from "@evinvest/uikit";
import { Archive, ArchiveRestore, Link, LogOut, Maximize, PanelRightOpen, PencilLine, PictureInPicture2, Pin, PinOff, Trash2 } from "lucide-react";
import { createContext, useContext } from "react";

import { type ChatSummary, archiveChat, chatLink, pinChat, assistant } from "@/entities/assistant";
import { type T, useLocale, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

import type { Display } from "../model/chat";
import { ago } from "./time";

export type Confirm = { kind: "delete" | "leave"; chat: ChatSummary } | { kind: "clear" };

/** What the surface a chat is shown on does with the actions taken on it. */
export interface Controls {
  display: Display;
  /** `id` moved into `display`. */
  onDisplay: (id: string, display: Display) => void;
  onConfirm: (confirm: Confirm) => void;
  /** A write landed: what the surface reads is read again. */
  onChanged: () => void;
}

export const ControlsContext = createContext<Controls | null>(null);

export function useControls(): Controls {
  const c = useContext(ControlsContext);
  if (c === null) throw new Error("chat actions are rendered inside a ChatView");
  return c;
}

export function copyLink(id: string, t: T): void {
  void navigator.clipboard.writeText(chatLink(id)).then(
    () => toast.positive(t(assistant.linkCopied, "Link copied")),
    (e: unknown) => notifyFailure(e, t),
  );
}

const DISPLAYS = [
  { id: "sidebar", icon: PanelRightOpen, name: (t: T) => t(assistant.display.sidebar, "Sidebar") },
  { id: "floating", icon: PictureInPicture2, name: (t: T) => t(assistant.display.floating, "Floating") },
  { id: "full", icon: Maximize, name: (t: T) => t(assistant.display.full, "Full screen") },
] as const;

/** The ⋯ menu's items, for the open chat and for a history row alike. `pinOutside`: Pin is a header button of its own. */
export function ChatActions({ chat, onRename, pinOutside = false }: { chat: ChatSummary; onRename: () => void; pinOutside?: boolean }) {
  const t = useT();
  const locale = useLocale();
  const { display, onDisplay, onConfirm, onChanged } = useControls();
  const interacts = chat.role !== "viewer";
  const write = (p: Promise<unknown>) => void p.then(onChanged, (e: unknown) => notifyFailure(e, t));
  return (
    <>
      <DropdownMenuItem onClick={() => copyLink(chat.id, t)}>
        <Link aria-hidden />
        {t(assistant.copyLink, "Copy link")}
        <DropdownMenuShortcut>Ctrl+Alt+L</DropdownMenuShortcut>
      </DropdownMenuItem>
      {interacts && !pinOutside && (
        <DropdownMenuItem onClick={() => write(pinChat(chat.id, chat.pinned_at === null))}>
          {chat.pinned_at === null ? <Pin aria-hidden /> : <PinOff aria-hidden />}
          {(chat.pinned_at === null ? t(assistant.pin, "Pin") : t(assistant.unpin, "Unpin"))}
        </DropdownMenuItem>
      )}
      {interacts && (
        <DropdownMenuItem onClick={onRename}>
          <PencilLine aria-hidden />
          {t(assistant.rename, "Rename")}
        </DropdownMenuItem>
      )}
      {interacts && (
        <DropdownMenuItem onClick={() => write(archiveChat(chat.id, chat.archived_at === null))}>
          {chat.archived_at === null ? <Archive aria-hidden /> : <ArchiveRestore aria-hidden />}
          {(chat.archived_at === null ? t(assistant.archive, "Archive") : t(assistant.unarchive, "Unarchive"))}
        </DropdownMenuItem>
      )}
      {chat.role === "owner" && (
        <DropdownMenuItem variant="destructive" onClick={() => onConfirm({ kind: "delete", chat })}>
          <Trash2 aria-hidden />
          {t(assistant.delete, "Delete")}
        </DropdownMenuItem>
      )}
      {chat.role === "member" && (
        <DropdownMenuItem variant="destructive" onClick={() => onConfirm({ kind: "leave", chat })}>
          <LogOut aria-hidden />
          {t(assistant.leave, "Leave")}
        </DropdownMenuItem>
      )}
      <DropdownMenuSeparator />
      <DropdownMenuRadioGroup value={display} onValueChange={(d) => onDisplay(chat.id, d as Display)}>
        {DISPLAYS.map(({ id, icon: Icon, name }) => (
          <DropdownMenuRadioItem key={id} value={id}>
            <Icon aria-hidden />
            {name(t)}
          </DropdownMenuRadioItem>
        ))}
      </DropdownMenuRadioGroup>
      <DropdownMenuSeparator />
      <p className="px-2 py-1.5 text-caption text-ink-soft">{t(assistant.updated, "Last updated {when}", { when: ago(chat.updated_at, locale) })}</p>
    </>
  );
}
