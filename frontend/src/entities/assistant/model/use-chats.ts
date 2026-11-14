"use client";

import { api, ok } from "@/shared/api";
import { useResource } from "@/shared/lib/use-resource";

import { fetchChat } from "../api/chats";

/** The caller's chats: owned and joined. */
export function useChats() {
  return useResource("assistant/chats", async () => (await ok(api.GET("/assistant/chats"))).chats, "assistant/chats", { live: ["assistant"], focus: true });
}

/** One chat; `null` for none asked, or one gone or not shared with the caller. */
export function useChat(id: string | null) {
  return useResource(`assistant/chat/${id}`, () => (id === null ? Promise.resolve(null) : fetchChat(id)), `assistant/chat/${id}`, { live: ["assistant"], focus: true });
}

/** Who may read the chat, and how. */
export function useAccess(id: string) {
  return useResource(`assistant/access/${id}`, () => ok(api.GET("/assistant/chats/{id}/access", { params: { path: { id } } })), `assistant/access/${id}`, { live: ["assistant"] });
}

/** Whom the caller may invite: people they share a chat with, and the one whose full email `q` is. */
export function usePeople(q: string) {
  return useResource(`assistant/people/${q}`, async () => (await ok(api.GET("/assistant/people", { params: { query: { q } } }))).people, "assistant/people");
}
