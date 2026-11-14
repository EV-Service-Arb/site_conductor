"use client";

import { api, ok } from "@/shared/api";
import { useResource } from "@/shared/lib/use-resource";

export function useAlerts() {
  return useResource("alerts", () => ok(api.GET("/alerts")), "alerts", { live: ["alerts"], focus: true });
}
