import type { Turn } from "@/shared/api/schema";
import { ROUTES } from "@/shared/config/routes";

export type AssistantTurn = Extract<Turn, { role: "assistant" }>;
export type UserTurn = Extract<Turn, { role: "user" }>;

/** Where a chat is opened by anyone it is shared with. */
export const chatLink = (id: string) => `${window.location.origin}${ROUTES.assistant}?chat=${id}`;
