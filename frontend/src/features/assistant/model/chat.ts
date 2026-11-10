"use client";

import { useSyncExternalStore } from "react";

import { type AssistantTurn, type Chat, type Turn, type UserTurn, createChat, fetchChat, putTurns } from "@/entities/assistant";
import { ApiError } from "@/shared/api";
import { setQuery } from "@/shared/lib/query";

/** `ask`: actions wait for a press; `act`: the first is taken at once, and can be undone. */
export type Approval = "ask" | "act";

/** Where the assistant sits beside the page; full screen is its own page, `/assistant/`. */
export type Mode = "sidebar" | "floating";
export type Display = Mode | "full";

/** The answer being written in this tab: its question and itself, from turn `from` of `chat`, until the server has both. */
export interface Running {
  chat: string;
  from: number;
  asked: UserTurn;
  answer: AssistantTurn;
  /** The chat's version once the settled answer is written: the server's copy shows it from then. */
  written: number | null;
}

interface Prefs {
  width: number;
  approval: Approval;
  /** The last shown, which the toggle opens; the one open is `?assistant=`. */
  mode: Mode;
  /** The chat in the sidebar or the floating window; `null` for a new one. */
  current: string | null;
  /** Something happened in a chat while folded. */
  unread: boolean;
}

interface State extends Prefs {
  running: Running | null;
}

const KEY = "sa:assistant/3";
const PREFS = ["width", "approval", "mode", "current", "unread"] as const satisfies readonly (keyof Prefs)[];
export const WIDTH = { min: 340, max: 760, initial: 420 } as const;
const INITIAL: State = { width: WIDTH.initial, approval: "ask", mode: "sidebar", current: null, unread: false, running: null };

let state: State | null = null;
const listeners = new Set<() => void>();

function current(): State {
  if (state === null) {
    const kept = localStorage.getItem(KEY);
    state = kept === null ? INITIAL : { ...(JSON.parse(kept) as Prefs), running: null };
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
  setQuery({ assistant: mode });
  if (mode !== null) set((s) => ({ ...s, mode, unread: false }));
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
/** The last answer's write: a question asked meanwhile is written after it, on the version it left. */
let settling: Promise<{ chat: string; version: number } | null> = Promise.resolve(null);

export function stop(): void {
  stopping?.abort();
}

/**
 * `asked` put at turn `from` of `chat` (a new chat for `null`), everything after it dropped, and answered by `play`
 * through `put`. Answers the chat's id at once, and `written`: whether the question reached the server. Once settled
 * the answer is written after it; `failed` hears why it was not (a conflict: someone wrote in the chat after the
 * question). One answer runs at a time, across chats; it goes on while the assistant is folded or another chat is open.
 */
export function ask(
  chat: Chat | null,
  from: number,
  asked: Omit<UserTurn, "role" | "id" | "at">,
  play: (put: (change: (t: AssistantTurn) => AssistantTurn) => void, signal: AbortSignal) => Promise<void>,
  failed: (e: unknown) => void,
): { id: string; written: Promise<void> } {
  stopping?.abort();
  const id = chat?.id ?? newId();
  const at = Date.now();
  const question: UserTurn = { role: "user", id: newId(), at, ...asked };
  let answer: AssistantTurn = { role: "assistant", id: newId(), at, ended: null, worked: null, steps: [], text: "", sources: [], actions: [], state: "working", feedback: null };
  const mine = (r: Running | null) => r?.answer.id === answer.id;
  const put = (change: (t: AssistantTurn) => AssistantTurn) => {
    answer = change(answer);
    updateRunning((r) => (mine(r) ? { ...r, answer } : r));
  };
  set((s) => ({ ...s, running: { chat: id, from, asked: question, answer, written: null } }));
  const run = new AbortController();
  stopping = run;
  const wrote = settling.then((last) =>
    chat === null ? createChat(id, titleOf(question.text), [question]) : putTurns(id, last?.chat === id ? Math.max(last.version, chat.version) : chat.version, from, [question]),
  );
  const written = wrote.then(
    () => undefined,
    (e: unknown) => {
      run.abort();
      set((s) => ({ ...s, running: mine(s.running) ? null : s.running }));
      throw e;
    },
  );
  settling = (async () => {
    const base = await wrote.then(
      (c) => c.version,
      // The question's own failure is `written`'s to tell.
      () => null,
    );
    await play(put, run.signal);
    if (stopping === run) stopping = null;
    if (base === null) return null;
    try {
      const saved = await putTurns(id, base, from + 1, [answer]).catch(async (e: unknown) => {
        if (!(e instanceof ApiError && e.failure.kind === "conflict")) throw e;
        const now = await fetchChat(id);
        if (now === null || now.turns.length !== from + 1 || now.turns[from]?.id !== question.id) throw e;
        return putTurns(id, now.version, from + 1, [answer]);
      });
      updateRunning((r) => (mine(r) ? { ...r, written: saved.version } : r));
      if (answer.state === "done") notice();
      return { chat: id, version: saved.version };
    } catch (e) {
      set((s) => ({ ...s, running: mine(s.running) ? null : s.running }));
      failed(e);
      return null;
    }
  })();
  return { id, written };
}

function titleOf(text: string): string {
  const line = text.replace(/\s+/g, " ").replace(/(^|\s)@/g, "$1").trim();
  return line.length > 48 ? `${line.slice(0, 47).trimEnd()}…` : line;
}
