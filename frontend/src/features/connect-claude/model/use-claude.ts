"use client";

import { api, ok } from "@/shared/api";
import { useResource } from "@/shared/lib/use-resource";

/** The caller's own Claude; `null` while their answers run on the panel's. */
export function useClaude() {
  return useResource("me/claude", () => ok(api.GET("/me/claude")), "me/claude", { live: ["me"] });
}
