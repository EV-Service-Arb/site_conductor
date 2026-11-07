"use client";

import { useT } from "@/shared/i18n";
import { setQuery, useQuery } from "@/shared/lib/query";

import { useKeymap } from "../model/keymap";
import { copyLink } from "./chat-actions";
import { ChatView } from "./chat-view";

const open = (id: string | null) => setQuery({ chat: id });

/** The assistant full screen: its own page, the chat in the URL (`?chat=`) for a link to open. */
export function AssistantPage() {
  const t = useT();
  const id = useQuery("chat");
  useKeymap({ toggle: null, newChat: () => open(null), copyLink: id !== null ? () => copyLink(id, t) : null });
  return <ChatView id={id} display="full" onOpen={open} onHide={null} />;
}
