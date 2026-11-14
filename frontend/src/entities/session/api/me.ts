import { http } from "@/shared/api";

/**
 * Ends the panel's session. `keepalive`: the concierge chip navigates to the platform sign-out
 * right after its `signout` event, and that unload must not cancel this POST.
 */
export async function signOut(): Promise<void> {
  await http.send("POST", "/auth/logout", undefined, () => null, undefined, { keepalive: true });
}
