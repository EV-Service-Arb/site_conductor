"use client";

import { useResource } from "@evinvest/api/react";

import { api } from "@/shared/api";
import type { Claude, OpenAi } from "@/shared/api/schema";

export type OwnModels = {
  claude: Claude | null;
  openai: OpenAi | null;
  /** Neither: their answers run on the panel's. */
  none: boolean;
  refused: boolean;
  reload: () => void;
};

/** The caller's own models; `null` until they are read. */
export function useOwnModels(): OwnModels | null {
  const claude = useResource(api, "/me/claude");
  const openai = useResource(api, "/me/openai");
  if (claude.status !== "ok" || openai.status !== "ok") return null;
  const own = [claude.data, openai.data].filter((m) => m !== null);
  return {
    claude: claude.data,
    openai: openai.data,
    none: own.length === 0,
    refused: own.some((m) => m.checked.error !== null),
    reload: () => {
      claude.reload();
      openai.reload();
    },
  };
}
