"use client";

import { StatusItem } from "@evinvest/uikit";
import Link from "next/link";

import { type Gmail, fetchConnections, useGmails } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { useResource } from "@/shared/lib/use-resource";

/** The hotbar's running work: each browser up, opening its gmail's Browser tab. Only a gmail whose browser passed its check can start one. */
export function RunningBrowsers() {
  const gmails = useGmails();
  if (gmails.status !== "ok") return null;
  return gmails.data.gmails.filter((g) => g.browser).map((g) => <Running key={g.id} gmail={g} />);
}

function Running({ gmail }: { gmail: Gmail }) {
  const t = useT();
  const connections = useResource(`connections.${gmail.id}`, () => fetchConnections(gmail.id), "connections", { live: "browsers" });
  if (connections.status !== "ok" || connections.data.browser?.running == null) return null;
  return (
    <Link href={`/?account=${gmail.id}&tab=browser`} className="rounded outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
      <StatusItem tone="positive">{t("Browser · {gmail}", { gmail: gmail.gmail })}</StatusItem>
    </Link>
  );
}
