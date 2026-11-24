"use client";

import { useResource } from "@evinvest/api/react";

import { useGmbMail } from "@/entities/mail";
import { api } from "@/shared/api";
import type { LoggedDto, MailLine } from "@/shared/api/schema";

type Moved = Extract<LoggedDto, { kind: "state" }>;

/** The latest word on a GBP's standing: a move of its state, or Google's own mail about it; `null` of none, `undefined` until read. */
export function useLastWord(gmb: number): Moved | MailLine | null | undefined {
  const log = useResource(api, "/gmbs/{id}/log", { params: { path: { id: gmb } } });
  const mail = useGmbMail(gmb);
  if (log.status !== "ok" || mail.status !== "ok") return undefined;
  const moved = log.data.events.find((e) => e.kind === "state");
  const line = mail.data.lines.find((l) => (l.who === "google" || l.who === "support") && l.kind !== "noise");
  if (moved === undefined || line === undefined) return moved ?? line ?? null;
  return Date.parse(line.at) > Date.parse(moved.at) ? line : moved;
}
