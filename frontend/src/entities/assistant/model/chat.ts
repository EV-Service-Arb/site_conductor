import { ROUTES } from "@/shared/config/routes";
import { type Parser, arrayOf, bool, nullable, num, object, oneOf, str } from "@/shared/lib/parse";

import type { Access, Chat, ChatSummary, Chats, People, Person, Turn } from "./generated";

export type { Access, Action, Attachment, Chat, ChatSummary, GeneralAccess, Mention, Person, Role, Source, Step, Turn } from "./generated";
export type AssistantTurn = Extract<Turn, { role: "assistant" }>;
export type UserTurn = Extract<Turn, { role: "user" }>;

const personParser: Parser<Person> = object({ id: str, name: str, email: str });
const general = oneOf(["none", "view"] as const);

const userTurnParser: Parser<UserTurn> = object({
  role: oneOf(["user"] as const),
  id: str,
  at: num,
  text: str,
  mentions: arrayOf(object({ kind: oneOf(["location"] as const), id: num, label: str })),
  files: arrayOf(object({ name: str, preview: nullable(object({ url: str, width: num, height: num })), bytes: num })),
});

const assistantTurnParser: Parser<AssistantTurn> = object({
  role: oneOf(["assistant"] as const),
  id: str,
  at: num,
  ended: nullable(num),
  worked: nullable(num),
  steps: arrayOf(object({ label: str, done: bool })),
  text: str,
  sources: arrayOf(object({ label: str, href: nullable(str) })),
  actions: arrayOf(object({ id: str, label: str, detail: str, href: nullable(str), done: str, state: oneOf(["proposed", "done"] as const) })),
  state: oneOf(["working", "writing", "done", "stopped"] as const),
  feedback: nullable(oneOf(["up", "down"] as const)),
});

const turnParser: Parser<Turn> = (v, path) =>
  typeof v === "object" && v !== null && "role" in v && v.role === "user" ? userTurnParser(v, path) : assistantTurnParser(v, path);

const summaryShape = {
  id: str,
  title: nullable(str),
  owner: personParser,
  role: oneOf(["owner", "member", "viewer"] as const),
  shared: bool,
  pinned_at: nullable(str),
  archived_at: nullable(str),
  updated_at: str,
};

export const chatsParser: Parser<Chats> = object({ chats: arrayOf<ChatSummary>(object(summaryShape)) });
export const chatParser: Parser<Chat> = object({ ...summaryShape, general, version: num, turns: arrayOf(turnParser) });
export const accessParser: Parser<Access> = object({ owner: personParser, members: arrayOf(personParser), general });
export const peopleParser: Parser<People> = object({ people: arrayOf(personParser) });

/** Where a chat is opened by anyone it is shared with. */
export const chatLink = (id: string) => `${window.location.origin}${ROUTES.assistant}?chat=${id}`;
