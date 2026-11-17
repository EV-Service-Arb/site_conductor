import type { Turn } from "@/shared/api/schema";
import { href } from "@evinvest/views";

import { VIEWS } from "@/shared/api/views";

export type AssistantTurn = Extract<Turn, { role: "assistant" }>;
export type UserTurn = Extract<Turn, { role: "user" }>;

/** Where a chat is opened by anyone it is shared with. */
export const chatLink = (id: string) => `${window.location.origin}${href(VIEWS, { view: "assistant", params: { chat: id } })}`;
