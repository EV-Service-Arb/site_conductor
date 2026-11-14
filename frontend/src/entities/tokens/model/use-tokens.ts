"use client";

import { api, ok } from "@/shared/api";
import { useResource } from "@/shared/lib/use-resource";

/** The caller's tokens; on focus too: a charge is no write of theirs, so no topic tells of it. */
export function useTokens() {
  return useResource("tokens", () => ok(api.GET("/tokens")), "tokens", { live: ["tokens"], focus: true });
}
