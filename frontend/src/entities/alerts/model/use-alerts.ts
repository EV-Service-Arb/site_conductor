"use client";

import { useResource } from "@evinvest/api/react";

import { api } from "@/shared/api";

export function useAlerts() {
  return useResource(api, "/alerts");
}
