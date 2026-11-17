"use client";

import { useResource } from "@evinvest/api/react";

import { api } from "@/shared/api";

/** The caller's chats: owned and joined. */
export function useChats() {
  return useResource(api, "/assistant/chats");
}

/** One chat; `null` when gone or not shared with the caller (the two answer alike). */
export function useChat(id: string) {
  const read = useResource(api, "/assistant/chats/{id}", { params: { path: { id } } });
  if (read.status === "error" && read.failure.code === "not_found") return { status: "ok" as const, data: null, fresh: read.fresh, reload: read.reload };
  return read;
}

/** Who may read the chat, and how. */
export function useAccess(id: string) {
  return useResource(api, "/assistant/chats/{id}/access", { params: { path: { id } } });
}

/** Whom the caller may invite: people they share a chat with, and the one whose full email `q` is. */
export function usePeople(q: string) {
  return useResource(api, "/assistant/people", { params: { query: { q } }, keep: true });
}
