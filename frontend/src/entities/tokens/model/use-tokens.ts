"use client";

import { useResource } from "@evinvest/api/react";

import { api } from "@/shared/api";

export function useTokens() {
  return useResource(api, "/tokens");
}
