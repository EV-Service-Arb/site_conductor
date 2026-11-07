"use client";

import {
  Avatar,
  AvatarFallback,
  Button,
  Command,
  CommandEmpty,
  CommandGroup,
  CommandItem,
  CommandList,
  Popover,
  PopoverContent,
  PopoverTrigger,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Spinner,
} from "@evinvest/uikit";
import { Building2, Link, X } from "lucide-react";
import { type ReactNode, useEffect, useId, useState } from "react";

import { type Access, type ChatSummary, type GeneralAccess, type Person, invite, removeMember, setGeneral, useAccess, usePeople } from "@/entities/assistant";
import { useMe } from "@/entities/session";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

import { copyLink, useControls } from "./chat-actions";

/** Who the chat is shared with, opened from `children`: inviting, removing, general access, the link. */
export function Share({ chat, children }: { chat: ChatSummary; children: ReactNode }) {
  return (
    <Popover>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align="end" className="flex w-110 flex-col gap-3 rounded-xl p-4">
        <Sharing chat={chat} />
      </PopoverContent>
    </Popover>
  );
}

function Sharing({ chat }: { chat: ChatSummary }) {
  const t = useT();
  const access = useAccess(chat.id);
  const { onChanged } = useControls();
  const changed = () => {
    access.reload();
    onChanged();
  };
  return (
    <>
      {chat.role !== "viewer" && access.status === "ok" && <Invite chat={chat.id} access={access.data} onInvited={changed} />}
      {access.status === "loading" && <Spinner className="size-4 self-center" />}
      {access.status === "error" && <p className="text-body-s text-negative">{t("assistant.share.failed")}</p>}
      {access.status === "ok" && <People chat={chat} access={access.data} onChanged={changed} />}
      <div className="-mx-4 -mb-4 flex justify-end border-t border-border-subtle px-3 py-2">
        <Button size="sm" variant="ghost" onClick={() => copyLink(chat.id, t)}>
          <Link aria-hidden />
          {t("assistant.copyLink")}
        </Button>
      </div>
    </>
  );
}

function Invite({ chat, access, onInvited }: { chat: string; access: Access; onInvited: () => void }) {
  const t = useT();
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [picking, setPicking] = useState(false);
  const [picked, setPicked] = useState<Person[]>([]);
  const [sending, setSending] = useState(false);
  const list = useId();
  useEffect(() => {
    const id = setTimeout(() => setQuery(search.trim()), 200);
    return () => clearTimeout(id);
  }, [search]);
  const people = usePeople(query);
  const taken = new Set([access.owner.id, ...access.members.map((m) => m.id), ...picked.map((p) => p.id)]);
  const found = people.status === "ok" ? people.data.filter((p) => !taken.has(p.id)) : [];
  const share = () => {
    setSending(true);
    void invite(
      chat,
      picked.map((p) => p.id),
    )
      .then(() => {
        setPicked([]);
        onInvited();
      }, (e: unknown) => notifyFailure(e, t))
      .finally(() => setSending(false));
  };
  return (
    <div className="relative flex gap-2">
      <Command
        shouldFilter={false}
        search={search}
        onSearchChange={setSearch}
        className="h-auto min-w-0 flex-1 overflow-visible bg-transparent"
        onFocus={() => setPicking(true)}
        onBlur={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setPicking(false);
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape" && picking) {
            e.preventDefault();
            setPicking(false);
          } else if (e.key === "Backspace" && search === "" && picked.length > 0) setPicked((p) => p.slice(0, -1));
        }}
      >
        <div className="flex min-h-8 flex-wrap items-center gap-1 rounded-md border border-border px-1.5 py-1 focus-within:ring-2 focus-within:ring-ring">
          {picked.map((p) => (
            <span key={p.id} className="inline-flex h-6 items-center gap-1 rounded-chip bg-hover pr-0.5 pl-2 text-body-s text-ink">
              {p.name}
              <button
                type="button"
                aria-label={t("assistant.share.unpick", { name: p.name })}
                onClick={() => setPicked((x) => x.filter((y) => y.id !== p.id))}
                className="rounded-sm p-0.5 text-ink-faint outline-none hover:bg-card hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
              >
                <X aria-hidden className="size-3" />
              </button>
            </span>
          ))}
          <input
            type="text"
            role="combobox"
            aria-expanded={picking}
            aria-controls={list}
            aria-label={t("assistant.share.invite")}
            placeholder={picked.length === 0 ? t("assistant.share.invite") : undefined}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="h-6 min-w-24 flex-1 bg-transparent px-1 text-body-s text-ink outline-none placeholder:text-ink-faint"
          />
        </div>
        {picking && (
          <CommandList id={list} className="absolute inset-x-0 top-full z-10 mt-1 max-h-60 rounded-xl border border-border bg-popover p-1 shadow-overlay">
            <CommandGroup heading={t("assistant.share.pick")}>
              {found.map((p) => (
                <CommandItem
                  key={p.id}
                  value={p.id}
                  onSelect={() => {
                    setPicked((x) => [...x, p]);
                    setSearch("");
                  }}
                >
                  <PersonRow person={p} />
                </CommandItem>
              ))}
            </CommandGroup>
            <CommandEmpty>{t(query.includes("@") ? "assistant.share.nobody" : "assistant.share.typeEmail")}</CommandEmpty>
          </CommandList>
        )}
      </Command>
      <Button size="sm" className="h-8" disabled={picked.length === 0 || sending} onClick={share}>
        {t("assistant.share.send")}
      </Button>
    </div>
  );
}

function People({ chat, access, onChanged }: { chat: ChatSummary; access: Access; onChanged: () => void }) {
  const t = useT();
  const me = useMe();
  const { onConfirm } = useControls();
  const owner = chat.role === "owner";
  const write = (p: Promise<unknown>) => void p.then(onChanged, (e: unknown) => notifyFailure(e, t));
  return (
    <>
      <section className="flex flex-col gap-1.5">
        <h3 className="text-caption font-semibold text-ink-soft">{t("assistant.share.people")}</h3>
        {[access.owner, ...access.members].map((p) => (
          <div key={p.id} className="flex items-center gap-2.5">
            <PersonRow person={p} you={p.id === me.user_id} />
            {p.id === access.owner.id ? (
              <span className="shrink-0 text-body-s text-ink-soft">{t("assistant.share.owner")}</span>
            ) : p.id === me.user_id ? (
              <Button size="sm" variant="ghost" onClick={() => onConfirm({ kind: "leave", chat })}>
                {t("assistant.leave")}
              </Button>
            ) : owner ? (
              <Select value="interact" onValueChange={(v) => v === "remove" && write(removeMember(chat.id, p.id))}>
                <SelectTrigger size="sm" className="shrink-0 border-0 bg-transparent">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="end">
                  <SelectItem value="interact">{t("assistant.share.interact")}</SelectItem>
                  <SelectItem value="remove">{t("assistant.share.remove")}</SelectItem>
                </SelectContent>
              </Select>
            ) : (
              <span className="shrink-0 text-body-s text-ink-soft">{t("assistant.share.interact")}</span>
            )}
          </div>
        ))}
      </section>
      <section className="flex flex-col gap-1.5">
        <h3 className="text-caption font-semibold text-ink-soft">{t("assistant.share.general")}</h3>
        <div className="flex items-center gap-2.5">
          <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-hover">
            <Building2 aria-hidden className="size-4 text-ink-mid" />
          </span>
          <span className="min-w-0 flex-1 truncate text-body-s text-ink">{t("assistant.share.everyone")}</span>
          {owner ? (
            <Select value={access.general} onValueChange={(v) => write(setGeneral(chat.id, v as GeneralAccess))}>
              <SelectTrigger size="sm" className="shrink-0 border-0 bg-transparent">
                <SelectValue />
              </SelectTrigger>
              <SelectContent align="end">
                <SelectItem value="view">{t("assistant.share.view")}</SelectItem>
                <SelectItem value="none">{t("assistant.share.none")}</SelectItem>
              </SelectContent>
            </Select>
          ) : (
            <span className="shrink-0 text-body-s text-ink-soft">{t(access.general === "view" ? "assistant.share.view" : "assistant.share.none")}</span>
          )}
        </div>
      </section>
    </>
  );
}

function PersonRow({ person, you = false }: { person: Person; you?: boolean }) {
  const t = useT();
  const initials = person.name
    .split(/\s+/)
    .map((w) => w[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();
  return (
    <span className="flex min-w-0 flex-1 items-center gap-2.5">
      <Avatar className="size-8 shrink-0">
        <AvatarFallback className="text-caption">{initials}</AvatarFallback>
      </Avatar>
      <span className="flex min-w-0 flex-col">
        <span className="truncate text-body-s text-ink">
          {person.name}
          {you && <span className="ml-1 text-caption text-ink-soft">{t("assistant.share.you")}</span>}
        </span>
        <span className="truncate text-caption text-ink-soft">{person.email}</span>
      </span>
    </span>
  );
}
