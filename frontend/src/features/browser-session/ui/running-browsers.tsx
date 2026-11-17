"use client";

import { StatusItem } from "@evinvest/uikit";

import { useGmails, browser } from "@/entities/gmb";
import { api } from "@/shared/api";
import type { Gmail } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { useResource } from "@evinvest/api/react";

/** The hotbar's running work: each browser up, opening its gmail's Browser tab. Only a gmail whose browser passed its check can start one. */
export function RunningBrowsers() {
  const gmails = useGmails();
  if (gmails.status !== "ok") return null;
  return gmails.data.gmails.filter((g) => g.browser).map((g) => <Running key={g.id} gmail={g} />);
}

function Running({ gmail }: { gmail: Gmail }) {
  const t = useT();
  const connections = useResource(api, "/gmails/{id}/connections", { params: { path: { id: gmail.id } }, keep: true });
  if (connections.status !== "ok" || connections.data.browser?.running == null) return null;
  return (
    <ViewLink to={{ view: "home", params: { account: gmail.id, tab: "browser" } }} className="rounded outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
      <StatusItem tone="positive">{t(browser.status, "Browser · {gmail}", { gmail: gmail.gmail })}</StatusItem>
    </ViewLink>
  );
}
