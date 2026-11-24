"use client";

import { useResource } from "@evinvest/api/react";
import { ChevronRight } from "lucide-react";

import { TOPIC_NAME, mail } from "@/entities/mail";
import { api } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";

/** Which of a gmail's replies go by themselves, and where that is set. */
export function AutoLink({ gmail }: { gmail: number }) {
  const t = useT();
  const auto = useResource(api, "/mail/auto");
  if (auto.status !== "ok") return null;
  const on = auto.data.on.filter((x) => x.gmail_id === gmail).map((x) => TOPIC_NAME[x.topic](t));
  return (
    <ViewLink to={{ view: "email", params: { show: "automatic" } }} className="flex items-center gap-2 rounded-md py-1.5 text-sm text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
      <span className="min-w-0 flex-1 truncate">{t(mail.tab.automatic, "Automatic replies")}</span>
      <span className="truncate text-xs text-ink-soft">{on.length === 0 ? t(mail.auto.off, "off") : on.join(", ")}</span>
      <ChevronRight aria-hidden className="size-4 shrink-0 text-ink-soft" />
    </ViewLink>
  );
}
