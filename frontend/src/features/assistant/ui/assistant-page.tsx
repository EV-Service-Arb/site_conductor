"use client";

import { useT } from "@/shared/i18n";
import { go, useAt } from "@/shared/lib/location";

import { useKeymap } from "../model/keymap";
import { copyLink } from "./chat-actions";
import { ChatView } from "./chat-view";

const open = (id: string | null) => go({ view: "assistant", params: id === null ? {} : { chat: id } }, "replace");

/** The assistant full screen: its own page, the chat in the URL (`?chat=`) for a link to open. */
export function AssistantPage() {
  const t = useT();
  const at = useAt();
  if (at.view !== "assistant") throw new Error(`the assistant's page on ${at.view}`);
  const id = at.params.chat ?? null;
  useKeymap({ toggle: null, newChat: () => open(null), copyLink: id !== null ? () => copyLink(id, t) : null });
  return <ChatView id={id} display="full" onOpen={open} onHide={null} />;
}
