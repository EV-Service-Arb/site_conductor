import { readCsrf } from "@/shared/api";

/**
 * Ends the panel's session. `keepalive`: the concierge chip navigates to the platform sign-out
 * right after its `signout` event, and that unload must not cancel this POST. `/auth` is in no document.
 */
export async function signOut(): Promise<void> {
  const csrf = readCsrf(document.cookie);
  if (csrf === null) throw new Error("a sign-out with no CSRF cookie beside the session");
  const answer = await fetch("/auth/logout", { method: "POST", credentials: "same-origin", keepalive: true, headers: { "x-sa-csrf": csrf } });
  if (!answer.ok) throw new Error(`the sign-out answered ${answer.status}`);
}
