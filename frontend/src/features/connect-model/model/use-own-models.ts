"use client";

import { useResource } from "@evinvest/api/react";

import { api } from "@/shared/api";

/** The caller's own Claude; `null` while their answers run on the panel's. */
export function useClaude() {
  return useResource(api, "/me/claude");
}
