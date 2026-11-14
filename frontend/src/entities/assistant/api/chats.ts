import { ApiError, api, ok } from "@/shared/api";
import type { Chat } from "@/shared/api/schema";

/** `null`: gone, or not shared with the caller (the two answer alike). */
export async function fetchChat(id: string): Promise<Chat | null> {
  try {
    return await ok(api.GET("/assistant/chats/{id}", { params: { path: { id } } }));
  } catch (e) {
    if (e instanceof ApiError && e.failure.kind === "not_found") return null;
    throw e;
  }
}
