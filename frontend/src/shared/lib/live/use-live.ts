"use client";

import { useEffect, useRef } from "react";

import type { paths } from "@/shared/api/schema";

import type { LiveStatus } from "./client";
import type { LiveMessage } from "./protocol";
import { LIVE_EVENT, LIVE_STATUS_EVENT } from "./relay";

type Writable<P extends keyof paths> = [paths[P]["post"] | paths[P]["put"] | paths[P]["patch"] | paths[P]["delete"]] extends [undefined] ? never : P;
type FirstSegment<P> = P extends `/${infer S}/${string}` ? S : P extends `/${infer S}` ? S : never;

/** The conductor tells a write as its path's first segment; the rest of the socket is the elements'. */
export type LiveTopic = FirstSegment<{ [P in keyof paths]: Writable<P> }[keyof paths]>;

/**
 * Calls `onSignal` on a change on any of `topics`, on a `resync`, and when the socket comes back
 * after a drop (what changed meanwhile was not told). The same window events the elements read.
 */
export function followLive(target: EventTarget, topics: readonly LiveTopic[], onSignal: () => void): () => void {
  const onMessage = (e: Event) => {
    const message = (e as CustomEvent<LiveMessage>).detail;
    if (message.type === "resync" || (message.type === "changed" && topics.some((t) => t === message.topic))) onSignal();
  };
  let dropped = false;
  const onStatus = (e: Event) => {
    const { status } = (e as CustomEvent<{ status: LiveStatus }>).detail;
    if (status === "live") {
      if (dropped) onSignal();
      dropped = false;
    } else if (status !== "connecting") dropped = true;
  };
  target.addEventListener(LIVE_EVENT, onMessage);
  target.addEventListener(LIVE_STATUS_EVENT, onStatus);
  return () => {
    target.removeEventListener(LIVE_EVENT, onMessage);
    target.removeEventListener(LIVE_STATUS_EVENT, onStatus);
  };
}

/** `followLive` on `window`, a burst (a batch sends one change per event) folded into one call per `debounceMs`. */
export function useLiveSignal(topics: readonly LiveTopic[], onSignal: () => void, debounceMs = 400): void {
  const latest = useRef(onSignal);
  useEffect(() => {
    latest.current = onSignal;
  });

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const stop = followLive(window, topics, () => {
      timer ??= setTimeout(() => {
        timer = null;
        latest.current();
      }, debounceMs);
    });
    return () => {
      stop();
      if (timer !== null) clearTimeout(timer);
    };
    // `topics` is a fresh array every render; its members name what is followed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [topics.join(), debounceMs]);
}
