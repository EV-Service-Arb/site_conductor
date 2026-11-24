"use client";

import { useResource } from "@evinvest/api/react";
import { Skeleton } from "@evinvest/uikit";
import { ChevronRight } from "lucide-react";

import { useNow } from "@/entities/alerts";
import { goes, useGmails } from "@/entities/gmb";
import { WHO_NAME, mail } from "@/entities/mail";
import { api } from "@/shared/api";
import { useLocale, useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { when } from "@/shared/lib/time";
import { ErrorState } from "@/shared/ui/error-state";

import { Held, Reply } from "./reply";
import { Wanted } from "./wanted";
import { Likely } from "./which-gbp";
import { Why } from "./why";

/** A case read and answered in place: what Google said, what it wants, the reply; its thread is the email view's. */
export function Conversation({ id }: { id: number }) {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  const gmails = useGmails();
  const read = useResource(api, "/mail/cases/{id}", { params: { path: { id } } });
  if (read.status === "error") return <ErrorState failure={read.failure} onRetry={read.reload} />;
  if (read.status === "loading" || gmails.status !== "ok") return <Skeleton className="h-32" />;
  const { case: c, messages } = read.data;
  const gmail = gmails.data.gmails.find((g) => g.id === c.gmail_id);
  // removed with its cases: the read that follows answers 404
  if (gmail === undefined) return <Skeleton className="h-32" />;
  const said = messages.findLast((m) => !m.sent && m.reading !== null);
  const reply = { detail: read.data, gmail: gmail.gmail, onChanged: read.reload };
  return (
    <div className="flex flex-col gap-3">
      {said?.reading != null && (
        <p className="text-sm break-words text-ink">
          <span className="text-ink-soft">
            {WHO_NAME.google(t)} · {when(said.at, locale, now)}:
          </span>{" "}
          {said.reading.summary}
        </p>
      )}
      <Why c={c} />
      <Likely c={c} onChanged={read.reload} />
      <Wanted c={c} onChanged={read.reload} />
      {goes(c) && c.draft !== null ? <Held draft={c.draft} {...reply} /> : <Reply {...reply} />}
      <ViewLink to={{ view: "email", params: { case: c.id } }} className="flex items-center gap-1 self-start rounded-sm text-sm text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
        {t(mail.conversation.whole, "The whole thread")}
        <ChevronRight aria-hidden className="size-3.5" />
      </ViewLink>
    </div>
  );
}
