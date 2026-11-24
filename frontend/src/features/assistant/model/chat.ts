"use client";

import { useSyncExternalStore } from "react";

import type { AssistantTurn, UserTurn } from "@/entities/assistant";
import type { Approval, Chat, Question, Turn } from "@/shared/api/schema";
import { api, ok } from "@/shared/api";
import { type Carried, setCarried } from "@/shared/lib/location";
import { DESKTOP_QUERY } from "@/shared/lib/use-media-query";

export type { Approval };

/** Where the assistant sits beside the page; full screen is its own page, `/assistant/`. */
export type Mode = NonNullable<Carried["assistant"]>;
export type Display = Mode | "full";

/** The answer being written in this tab: its question and itself, from turn `from` of `chat`, until the server has both. */
export interface Running {
  chat: string;
  from: number;
  asked: UserTurn;
  answer: AssistantTurn;
  /** The chat's version that holds the question: the server's copy shows from then. */
  written: number | null;
}

interface Prefs {
  width: number;
  approval: Approval;
  /** The last shown, which the toggle opens; the one open is `?assistant=`. */
  mode: Mode;
  /** Folded by this browser's choice, which a page without `?assistant=` follows. */
  folded: boolean;
  /** The chat in the sidebar or the floating window; `null` for a new one. */
  current: string | null;
  /** Something happened in a chat while folded. */
  unread: boolean;
}

interface State extends Prefs {
  running: Running | null;
}

const KEY = "sa:assistant/4";
const PREFS = ["width", "approval", "mode", "folded", "current", "unread"] as const satisfies readonly (keyof Prefs)[];
export const WIDTH = { min: 340, max: 760, initial: 420 } as const;
const INITIAL: State = { width: WIDTH.initial, approval: "ask", mode: "sidebar", folded: false, current: null, unread: false, running: null };

let state: State | null = null;
const listeners = new Set<() => void>();

function current(): State {
  if (state === null) {
    const kept = localStorage.getItem(KEY);
    // on a phone it covers the page, so it waits to be asked for
    state = kept === null ? { ...INITIAL, folded: !window.matchMedia(DESKTOP_QUERY).matches } : { ...(JSON.parse(kept) as Prefs), running: null };
  }
  return state;
}

function set(change: (s: State) => State): void {
  const was = current();
  state = change(was);
  const next = state;
  if (PREFS.some((k) => next[k] !== was[k])) localStorage.setItem(KEY, JSON.stringify(Object.fromEntries(PREFS.map((k) => [k, next[k]]))));
  for (const l of listeners) l();
}

const subscribe = (onChange: () => void) => {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
};

export function useAssistant(): State {
  return useSyncExternalStore(subscribe, current, () => INITIAL);
}

export const newId = () => crypto.randomUUID();

/** The sidebar or the floating window, else (`null`) folded. */
export function show(mode: Mode | null): void {
  setCarried({ assistant: mode ?? undefined });
  set((s) => (mode === null ? { ...s, folded: true } : { ...s, mode, folded: false, unread: false }));
}

/** A page without `?assistant=` shows it as this browser last left it. */
export function restore(): void {
  const s = current();
  setCarried((c) => (c.assistant === undefined && !s.folded ? { assistant: s.mode } : {}));
}

export function setWidth(width: number): void {
  set((s) => ({ ...s, width: Math.round(Math.min(WIDTH.max, Math.max(WIDTH.min, width))) }));
}

export function setApproval(approval: Approval): void {
  set((s) => ({ ...s, approval }));
}

/** `chat` in the sidebar or the floating window, opened in `mode` when given. */
export function openChat(id: string | null, mode?: Mode): void {
  set((s) => ({ ...s, current: id }));
  if (mode) show(mode);
}

/** A new chat in the mode last shown. */
export function openNew(): void {
  openChat(null, current().mode);
}

/** Something happened in a chat: told on the toggle, and seen at once while the assistant is open. */
export function notice(): void {
  set((s) => ({ ...s, unread: true }));
}

/** Whether `running` stands in for the tail of chat `id`: until the server's copy has its answer, or the chat is gone. */
export function overlaid(chat: Chat | null, id: string | null, running: Running | null): running is Running {
  return running !== null && running.chat === id && (running.written === null || (chat !== null && chat.version < running.written));
}

/** What a chat shows: the server's turns, with the answer this tab is writing in place of their tail. */
export function shown(chat: Chat | null, id: string | null, running: Running | null): Turn[] {
  const turns = chat?.turns ?? [];
  return overlaid(chat, id, running) ? [...turns.slice(0, running.from), running.asked, running.answer] : turns;
}

function updateRunning(change: (r: Running) => Running): void {
  set((s) => ({ ...s, running: s.running && change(s.running) }));
}

let stopping: AbortController | null = null;

/** The answer this tab waits for stops: written as stopped where the conductor left it. */
export function stop(): void {
  stopping?.abort();
}

/** What the page tells the assistant: what is open, the bindings bound now. */
export type Page = Pick<Question, "url" | "bindings">;

/**
 * `asked` put at turn `from` of `chat` (a new chat for `null`), everything after it dropped, and answered by the
 * conductor, which writes the answer into the chat as it grows. Answers the chat's id at once; `answered` settles
 * with the chat once the answer is done, and rejects with why it was not.
 */
export function ask(chat: Chat | null, from: number, asked: Omit<UserTurn, "role" | "id" | "at">, page: Page, approval: Approval): { id: string; answered: Promise<Chat> } {
  stopping?.abort();
  const id = chat?.id ?? newId();
  const at = Date.now();
  const question: UserTurn = { role: "user", id: newId(), at, ...asked };
  const answer: AssistantTurn = { role: "assistant", id: newId(), at, ended: null, worked: null, steps: [], text: "", sources: [], actions: [], state: "working", feedback: null, cost: null, connect: false };
  const mine = (r: Running | null) => r?.answer.id === answer.id;
  set((s) => ({ ...s, running: { chat: id, from, asked: question, answer, written: null } }));
  const run = new AbortController();
  stopping = run;
  const made = chat === null ? ok(api.POST("/assistant/chats", { body: { id, title: titleOf(question.text), turns: [] } })) : Promise.resolve(chat);
  const answered = made
    .then((c) => {
      // The conductor's first write holds the question and the answer begun: the chat shows its own from then.
      updateRunning((r) => (mine(r) ? { ...r, written: c.version + 1 } : r));
      return ok(api.POST("/assistant/chats/{id}/ask", { params: { path: { id } }, body: { from, ...asked, ...page, approval }, signal: run.signal }));
    })
    .then(
      (done) => {
        set((s) => ({ ...s, running: mine(s.running) ? null : s.running }));
        notice();
        return done;
      },
      async (e: unknown) => {
        set((s) => ({ ...s, running: mine(s.running) ? null : s.running }));
        if (run.signal.aborted) return stopped(id);
        throw e;
      },
    );
  return { id, answered };
}

/** The chat's last answer, cut off where it stood. */
async function stopped(id: string): Promise<Chat> {
  const now = await ok(api.GET("/assistant/chats/{id}", { params: { path: { id } } }));
  const last = now.turns.length - 1;
  const turn = now.turns[last];
  if (turn?.role !== "assistant" || (turn.state !== "working" && turn.state !== "writing")) return now;
  const cut: AssistantTurn = { ...turn, state: "stopped", ended: Date.now(), worked: Date.now() - turn.at, steps: turn.steps.map((x) => ({ ...x, done: true })) };
  return ok(api.PUT("/assistant/chats/{id}/turns", { params: { path: { id } }, body: { base: now.version, from: last, turns: [cut] } }));
}

function titleOf(text: string): string {
  const line = text.replace(/\s+/g, " ").replace(/(^|\s)@/g, "$1").trim();
  return line.length > 48 ? `${line.slice(0, 47).trimEnd()}…` : line;
}
