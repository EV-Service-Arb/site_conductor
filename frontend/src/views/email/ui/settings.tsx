"use client";

import { Card, CardContent, CardHeader, CardTitle, Field, FieldDescription, FieldError, FieldLabel, Input, Skeleton, useUndo } from "@evinvest/uikit";
import { useId, useState } from "react";

import { mail, setMailSetting, useMailSettings } from "@/entities/mail";
import type { MailSettings, MailSettingsSet } from "@/shared/api/schema";
import { type T, useT } from "@/shared/i18n";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useControlSize } from "@/shared/ui/touch";

type Setting = keyof MailSettings;

const LABEL: Record<Setting, (t: T) => string> = {
  hold_minutes: (t) => t(mail.settings.hold, "How long a reply waits before it goes by itself, so you can stop it (minutes)"),
  apart_hours: (t) => t(mail.settings.apart, "Least time after our last email in a thread (hours)"),
  limit: (t) => t(mail.settings.limit, "Most emails to Google in a row with nothing new from it"),
  re_ask_hours: (t) => t(mail.settings.reAsk, "Ask again when Google is silent for (hours)"),
  in_review_days: (t) => t(mail.settings.inReview, "Ask again about a video or an appeal in review after (days)"),
  remind_hours: (t) => t(mail.settings.remind, "Remind me of what still waits on me after (hours)"),
  deadline_hours: (t) => t(mail.settings.deadline, "Warn me before a date Google set (hours)"),
};

const lowered = (t: T) => t(mail.settings.lowered, "Set lower, where automatic replies are on, every thread silent that long already gets its reply written at once, at its usual cost in tokens. It still waits before it goes, and you can stop it.");
const unlessTold = (t: T) => t(mail.settings.unlessTold, "Where Google said how long it takes, that counts instead.");

const NOTE: Partial<Record<Setting, (t: T) => string>> = {
  hold_minutes: (t) => t(mail.settings.hold.note, "A reply waiting already goes when it was said to."),
  limit: (t) => t(mail.settings.limit.note, "After that the next one is yours to send."),
  re_ask_hours: (t) => `${unlessTold(t)} ${lowered(t)}`,
  in_review_days: (t) => `${unlessTold(t)} ${lowered(t)}`,
};

const CARDS: { title: (t: T) => string; of: readonly Setting[] }[] = [
  { title: (t) => t(mail.tab.automatic, "Automatic replies"), of: ["hold_minutes", "apart_hours", "limit"] },
  { title: (t) => t(mail.settings.waiting, "Waiting for Google"), of: ["re_ask_hours", "in_review_days"] },
  { title: (t) => t(mail.settings.alerts, "Alerts"), of: ["remind_hours", "deadline_hours"] },
];

/** How Google is pursued in every thread of the caller's, whatever the outline has selected. */
export function Settings() {
  const t = useT();
  const read = useMailSettings();
  if (read.status === "error") return <ErrorState failure={read.failure} onRetry={read.reload} />;
  if (read.status === "loading") return <Skeleton className="h-60" />;
  return (
    <div className="flex w-full max-w-160 flex-col gap-4">
      <p className="text-sm text-ink-soft">{t(mail.settings.whole, "For all your gmails and GBPs.")}</p>
      {CARDS.map(({ title, of }) => (
        <Card key={of[0]} className="gap-3 py-4">
          <CardHeader className="px-4">
            <CardTitle className="text-xs font-medium uppercase tracking-wide text-ink-soft">{title(t)}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4 px-4">
            {of.map((s) => (
              <Edit key={`${s} ${read.data.set[s]}`} of={s} read={read.data} reload={read.reload} />
            ))}
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

function Edit({ of, read, reload }: { of: Setting; read: MailSettingsSet; reload: () => void }) {
  const t = useT();
  const id = useId();
  const size = useControlSize();
  const push = useUndo();
  const [least, set, most] = [read.least[of], read.set[of], read.most[of]];
  const [typed, setTyped] = useState(String(set));
  const n = Number(typed);
  const within = typed.trim() !== "" && Number.isInteger(n) && least <= n && n <= most;
  const write = (to: number) => setMailSetting(of, to).finally(reload);
  const save = async () => {
    if (!within || n === set) return;
    try {
      await write(n);
      push({ label: t(mail.settings.changed, "{setting}: {from} to {to}", { setting: LABEL[of](t), from: set, to: n }), undo: async () => void (await write(set)), redo: async () => void (await write(n)) });
    } catch (e) {
      notifyFailure(e, t);
      setTyped(String(set));
    }
  };
  const range = t(mail.settings.range, "{least} to {most}", { least, most });
  return (
    <Field className="flex flex-col gap-1">
      <FieldLabel htmlFor={id}>{LABEL[of](t)}</FieldLabel>
      <div className="flex items-center gap-3">
        <Input
          id={id}
          size={size}
          type="number"
          min={least}
          max={most}
          step={1}
          inputMode="numeric"
          className="w-24 tabular-nums"
          aria-describedby={`${id}-range`}
          aria-invalid={!within || undefined}
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          onBlur={() => void save()}
          onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
        />
        {within ? (
          <FieldDescription id={`${id}-range`} className="text-xs tabular-nums">
            {range}
          </FieldDescription>
        ) : (
          <FieldError id={`${id}-range`} className="text-xs tabular-nums">
            {t(mail.settings.beyond, "Not saved: {range}", { range })}
          </FieldError>
        )}
      </div>
      {NOTE[of] !== undefined && <FieldDescription className="text-xs">{NOTE[of](t)}</FieldDescription>}
    </Field>
  );
}
