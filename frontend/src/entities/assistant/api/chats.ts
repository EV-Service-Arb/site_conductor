import { ApiError, http, ignoreBody } from "@/shared/api";

import { type Access, type Chat, type ChatSummary, type GeneralAccess, type Person, type Turn, accessParser, chatParser, chatsParser, peopleParser } from "../model/chat";

const API = "/api/v1/assistant";

export async function fetchChats(): Promise<ChatSummary[]> {
  return (await http.get(`${API}/chats`, chatsParser)).chats;
}

/** `null`: gone, or not shared with the caller (the two answer alike). */
export async function fetchChat(id: string): Promise<Chat | null> {
  try {
    return await http.get(`${API}/chats/${id}`, chatParser);
  } catch (e) {
    if (e instanceof ApiError && e.failure.kind === "not_found") return null;
    throw e;
  }
}

export function createChat(id: string, title: string | null, turns: Turn[]): Promise<Chat> {
  return http.send("POST", `${API}/chats`, { id, title, turns }, chatParser);
}

/** The chat's turns from `from` on replaced by `turns`; a 409 when `base` is no longer its version. */
export function putTurns(id: string, base: number, from: number, turns: Turn[]): Promise<Chat> {
  return http.send("PUT", `${API}/chats/${id}/turns`, { base, from, turns }, chatParser);
}

export function renameChat(id: string, title: string): Promise<Chat> {
  return http.send("PUT", `${API}/chats/${id}/title`, { title }, chatParser);
}

export function pinChat(id: string, on: boolean): Promise<null> {
  return http.send("PUT", `${API}/chats/${id}/pinned`, { on }, ignoreBody);
}

export function archiveChat(id: string, on: boolean): Promise<null> {
  return http.send("PUT", `${API}/chats/${id}/archived`, { on }, ignoreBody);
}

export function deleteChat(id: string): Promise<null> {
  return http.send("DELETE", `${API}/chats/${id}`, undefined, ignoreBody);
}

export function clearArchived(): Promise<null> {
  return http.send("DELETE", `${API}/archived`, undefined, ignoreBody);
}

export function fetchAccess(id: string): Promise<Access> {
  return http.get(`${API}/chats/${id}/access`, accessParser);
}

export function invite(id: string, users: string[]): Promise<Access> {
  return http.send("POST", `${API}/chats/${id}/members`, { users }, accessParser);
}

export function removeMember(id: string, user: string): Promise<null> {
  return http.send("DELETE", `${API}/chats/${id}/members/${user}`, undefined, ignoreBody);
}

export function setGeneral(id: string, general: GeneralAccess): Promise<Access> {
  return http.send("PUT", `${API}/chats/${id}/general`, { general }, accessParser);
}

export async function fetchPeople(q: string): Promise<Person[]> {
  // Spelled out: the query helper drops an empty `q`, which the route requires.
  return (await http.get(`${API}/people?q=${encodeURIComponent(q)}`, peopleParser)).people;
}
