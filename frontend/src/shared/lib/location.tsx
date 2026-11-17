"use client";

import { type At as AtOf, type Carried as CarriedOf, ViewError, href, parse } from "@evinvest/views";
import { type AnchorHTMLAttributes, type MouseEvent, useSyncExternalStore } from "react";

import { VIEWS } from "@/shared/api/views";

export type At = AtOf<typeof VIEWS>;
export type Carried = CarriedOf<typeof VIEWS>;
export type Params<V extends At["view"]> = Extract<At, { view: V }>["params"];
export type Home = Params<"home">;

/** Where the page is: a view and what it carries, or a location that is none. */
export type Here = { at: At; carried: Carried } | { error: ViewError };

let shown: { location: string; here: Here } | null = null;
const listeners = new Set<() => void>();
let ours = false;

const location = () => window.location.pathname + window.location.search;

function parsed(location: string): Here {
  try {
    return parse(VIEWS, location);
  } catch (e) {
    if (e instanceof ViewError) return { error: e };
    throw e;
  }
}

function read(): Here {
  const now = location();
  if (shown?.location !== now) shown = { location: now, here: parsed(now) };
  return shown.here;
}

/** The elements route by the location and hear nothing of `pushState`: each move is announced as back and forward are (Next ignores a `popstate` without its state). */
function announce() {
  queueMicrotask(() => window.dispatchEvent(new PopStateEvent("popstate")));
}

function notify() {
  for (const l of listeners) l();
}

/** A move the store did not make (an element's, back, forward): the carried keys it dropped are kept. */
function moved(popped: boolean) {
  const was = shown?.here;
  const now = parsed(location());
  if (was !== undefined && "at" in was && "at" in now) {
    const kept = { ...was.carried, ...now.carried };
    if (Object.keys(kept).length !== Object.keys(now.carried).length) return write(href(VIEWS, now.at, kept), "replace");
  }
  if (shown?.location === location()) return;
  read();
  notify();
  if (!popped) announce();
}

function install() {
  const { pushState, replaceState } = window.history;
  window.history.pushState = function (...args: Parameters<History["pushState"]>) {
    pushState.apply(this, args);
    if (!ours) moved(false);
  };
  window.history.replaceState = function (...args: Parameters<History["replaceState"]>) {
    replaceState.apply(this, args);
    if (!ours) moved(false);
  };
  window.addEventListener("popstate", () => moved(true));
}
// before Next's router patches the history too: its wraps this one
if (typeof window !== "undefined") install();

function subscribe(onChange: () => void) {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
}

function write(to: string, how: "push" | "replace") {
  ours = true;
  try {
    if (how === "push") window.history.pushState(null, "", to);
    else window.history.replaceState(null, "", to);
  } finally {
    ours = false;
  }
  read();
  notify();
  announce();
}

/** `null` while the page is built: the export knows no location. */
export function useHere(): Here | null {
  return useSyncExternalStore(subscribe, read, () => null);
}

function viewed(): { at: At; carried: Carried } {
  const here = read();
  if ("error" in here) throw new Error(`no view is here: ${here.error.message}`);
  return here;
}

/** The view the page shows; only what a view renders asks, so there is one. */
export function useAt(): At {
  const here = useHere();
  if (here === null || "error" in here) throw new Error("useAt outside a view");
  return here.at;
}

export function useCarried(): Carried {
  const here = useHere();
  if (here === null || "error" in here) throw new Error("useCarried outside a view");
  return here.carried;
}

/** Home's own state, or `null` on any other view. */
export function useHome(): Home | null {
  const at = useAt();
  return at.view === "home" ? at.params : null;
}

/** `at`'s href with what the page carries now. */
export const linkTo = (at: At): string => href(VIEWS, at, viewed().carried);

/** To `at`, carried kept; or to an href that is a view (the assistant's, an element's link). */
export function go(to: At | string, how: "push" | "replace" = "push"): void {
  if (typeof to === "string") {
    const { at, carried } = parse(VIEWS, to);
    return write(href(VIEWS, at, { ...viewed().carried, ...carried }), how);
  }
  write(linkTo(to), how);
}

/** A change of some keys: `undefined` takes one out. */
type Change<T> = { [K in keyof T]?: T[K] | undefined };

function changed<T extends object>(now: T, change: Change<T>): T {
  return Object.fromEntries(Object.entries({ ...now, ...change }).filter(([, v]) => v !== undefined)) as T;
}

/** Home's keys changed: a window opened, the map moved. On Home the history entry is replaced, as a person does not step back through them; from any other view it is a move to Home. */
export function setHome(change: Change<Home> | ((now: Home) => Change<Home>)): void {
  const { at } = viewed();
  const now = at.view === "home" ? at.params : {};
  write(linkTo({ view: "home", params: changed(now, typeof change === "function" ? change(now) : change) }), at.view === "home" ? "replace" : "push");
}

export function setCarried(change: Change<Carried> | ((now: Carried) => Change<Carried>)): void {
  const { at, carried } = viewed();
  write(href(VIEWS, at, changed(carried, typeof change === "function" ? change(carried) : change)), "replace");
}

/** The location's path, for what matches links by it (the kit's nav menus). */
export function usePath(): string {
  return useSyncExternalStore(subscribe, () => window.location.pathname, () => "/");
}

/** A link to a view, by name or by its href: a plain click moves the store, any other the browser's. */
export function ViewLink({ to, href: given, onClick, ...rest }: ({ to: At; href?: never } | { href: string; to?: never }) & Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "href">) {
  const carried = useCarried();
  const target = to === undefined ? given : href(VIEWS, to, carried);
  const click = (e: MouseEvent<HTMLAnchorElement>) => {
    onClick?.(e);
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || rest.target === "_blank") return;
    e.preventDefault();
    go(target);
  };
  return <a href={target} onClick={click} {...rest} />;
}
