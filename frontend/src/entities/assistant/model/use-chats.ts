"use client";

import { useResource } from "@/shared/lib/use-resource";

import { fetchAccess, fetchChat, fetchChats, fetchPeople } from "../api/chats";

/** The caller's chats: owned and joined. */
export function useChats() {
  return useResource("assistant/chats", fetchChats, "assistant/chats", { live: "assistant", focus: true });
}

/** One chat; `null` for none asked, or one gone or not shared with the caller. */
export function useChat(id: string | null) {
  return useResource(`assistant/chat/${id}`, () => (id === null ? Promise.resolve(null) : fetchChat(id)), `assistant/chat/${id}`, { live: "assistant", focus: true });
}

/** Who may read the chat, and how. */
export function useAccess(id: string) {
  return useResource(`assistant/access/${id}`, () => fetchAccess(id), `assistant/access/${id}`, { live: "assistant" });
}

/** Whom the caller may invite: people they share a chat with, and the one whose full email `q` is. */
export function usePeople(q: string) {
  return useResource(`assistant/people/${q}`, () => fetchPeople(q), "assistant/people");
}
