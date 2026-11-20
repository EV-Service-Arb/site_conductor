// State of the sign-in overlay, outside React: a 401 is seen by the client and the live socket, which
// are not components. The overlay is `signinOverlay` in panel.config.ts; off, or
// with no host component mounted to show it, `requestSignInOverlay` says no and the caller
// redirects as it always did.

import { PANEL } from "@config";

export type OverlayState = "idle" | "open" | "dismissed";

let state: OverlayState = "idle";
let hosts = 0;
const listeners = new Set<() => void>();

function set(next: OverlayState): void {
  if (state === next) return;
  state = next;
  for (const l of listeners) l();
}

export function subscribeOverlay(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function overlayState(): OverlayState {
  return state;
}

/** A component that can show the overlay is mounted; without one a request must not be swallowed. */
export function registerOverlayHost(): () => void {
  hosts += 1;
  return () => {
    hosts -= 1;
  };
}

/** A dev sign-in backend's mark (`sa_server::cookies::DEV_SIGN_IN`): `/auth/login` signs in without concierge. */
function devSignIn(): boolean {
  return typeof document !== "undefined" && document.cookie.split(";").some(p => p.trim() === "sa_dev_sign_in=1");
}

/** True when the overlay took the 401; false means the caller must redirect itself. */
export function requestSignInOverlay(): boolean {
  if (!PANEL.signinOverlay || hosts === 0 || devSignIn()) return false;
  rememberPage();
  set("open");
  return true;
}

export function openOverlay(): void {
  rememberPage();
  set("open");
}

export function dismissOverlay(): void {
  set("dismissed");
}

// The dialog's OAuth round trip may only come back to a path the tenant registered (`/auth/login`),
// not to the page the person was on, so the page is kept here and put back after the callback.
const RETURN_KEY = "sa:return-after-signin";
const MAX_PATH = 512;

/** A path on this origin, nothing a browser reads as another (`//host`, `/\host`, `javascript:`); else null. */
export function safeLocalPath(raw: string | null): string | null {
  if (raw === null) return null;
  const ok = raw.length <= MAX_PATH && raw.startsWith("/") && raw[1] !== "/" && !raw.includes("\\") && /^[\x21-\x7e]+$/.test(raw);
  return ok ? raw : null;
}

function rememberPage(): void {
  if (typeof window === "undefined") return;
  try {
    const here = safeLocalPath(window.location.pathname + window.location.search);
    if (here !== null) window.sessionStorage.setItem(RETURN_KEY, here);
  } catch {
    // Storage blocked: the person lands on the start page, as before.
  }
}

/** The page to go back to after signing in, once: the key is cleared on read. Null: none, or not a safe path. */
export function takeReturnPage(): string | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = window.sessionStorage.getItem(RETURN_KEY);
    window.sessionStorage.removeItem(RETURN_KEY);
    return safeLocalPath(raw);
  } catch {
    return null;
  }
}

/** The backend's sign-in; a full navigation, since it answers with a redirect to concierge. */
export const SIGN_IN_PATH = "/auth/login";

/** The sign-in through Google's account chooser, past the evinvest.ltd session the browser may have. */
export function switchAccountPath(returnTo: string): string {
  return `${SIGN_IN_PATH}?prompt=select_account&return_to=${encodeURIComponent(returnTo)}`;
}

/** The full-page way to sign in: off to the backend's sign-in, and back here. */
export function redirectToSignIn(): void {
  if (typeof window === "undefined") return;
  const here = window.location.pathname + window.location.search;
  // /auth/login is the Rust backend's route, not a page of this app: only a full navigation reaches it.
  // eslint-disable-next-line @next/next/no-location-assign-relative-destination
  window.location.assign(`${SIGN_IN_PATH}?return_to=${encodeURIComponent(here)}`);
}

/** The session is gone (an HTTP 401, the live socket's 4401): the overlay when it is on, else the redirect. */
export function goToSignIn(): void {
  if (requestSignInOverlay()) return;
  redirectToSignIn();
}
