"use client";

import { Button, Popover, PopoverContent, PopoverTrigger, ScrollArea } from "@evinvest/uikit";
import { CircleHelp, Copy } from "lucide-react";
import type { ReactNode } from "react";

import { useT, account } from "@/shared/i18n";
import { useButtonSize } from "@/shared/ui/touch";

/** One step: what to open, and what to do there. */
export type Step = { text: ReactNode; href?: string };

/** Where to click to get a connection's credentials: a video when there is one, then the steps. */
export function ConnectHelp({ title, video, steps }: { title: string; video?: string; steps: Step[] }) {
  const t = useT();
  const button = useButtonSize();
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="ghost" size={button("sm")} className="gap-1 self-start px-1 text-ink-soft">
          <CircleHelp aria-hidden className="size-4" />
          {t(account.help.how, "How to get it")}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-[min(28rem,90vw)] p-0">
        <ScrollArea className="max-h-[70vh]">
          <div className="flex flex-col gap-3 p-4">
            <h4 className="text-sm font-semibold text-ink">{title}</h4>
            {video !== undefined && <video src={video} controls muted playsInline className="w-full rounded-md border border-border" />}
            <ol className="flex list-decimal flex-col gap-2 pl-5 text-sm text-ink">
              {steps.map((s, i) => (
                <li key={i}>
                  {s.href !== undefined ? (
                    <a href={s.href} target="_blank" rel="noreferrer" className="underline underline-offset-2">
                      {s.text}
                    </a>
                  ) : (
                    s.text
                  )}
                </li>
              ))}
            </ol>
          </div>
        </ScrollArea>
      </PopoverContent>
    </Popover>
  );
}

/** A value to paste somewhere else, with its copy button. */
export function Copyable({ value }: { value: string }) {
  const t = useT();
  return (
    <span className="inline-flex max-w-full items-center gap-1 align-middle">
      <code className="truncate rounded bg-muted px-1 font-mono text-xs">{value}</code>
      <button type="button" aria-label={t(account.help.copy, "Copy")} className="text-ink-soft hover:text-ink" onClick={() => void navigator.clipboard.writeText(value)}>
        <Copy aria-hidden className="size-3.5" />
      </button>
    </span>
  );
}
