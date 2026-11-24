"use client";

import { Button, Skeleton, Tabs, TabsList, TabsTrigger, cn, useBinding } from "@evinvest/uikit";
import { ArrowLeft } from "lucide-react";
import { type KeyboardEvent, useEffect, useRef } from "react";

import { useGmails } from "@/entities/gmb";
import { CaseRow, type Show, mail, over, shownIn, stuckOf, unnamed, useCases } from "@/entities/mail";
import { Mend, WhichGbp } from "@/features/answer-mail";
import { AutoReplies } from "@/features/auto-mail";
import type { MailCase } from "@/shared/api/schema";
import { type T, nav, useT } from "@/shared/i18n";
import { type Params, go, useAt } from "@/shared/lib/location";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";

import { CaseDetail } from "./case-detail";
import { Inboxes } from "./inboxes";

type Sought = Params<"email">;

/** Google's mail about the caller's GBPs, as cases: the list and the one opened, in a narrow room one or the other; and which replies go by themselves. */
export function EmailView() {
  const at = useAt();
  if (at.view !== "email") throw new Error("the email view on another's");
  const t = useT();
  const { case: opened, show } = at.params;
  const automatic = show === "automatic";
  return (
    <div className="@container flex min-h-0 flex-1 flex-col">
      <header className={cn("flex items-center gap-3 border-b border-border px-4", opened !== undefined && "@max-3xl:hidden")}>
        <h1 className="text-base font-semibold text-ink">{t(nav.email, "Email")}</h1>
        <Tabs value={automatic ? "automatic" : "threads"} onValueChange={(tab) => go({ view: "email", params: tab === "automatic" ? { show: "automatic" } : {} })}>
          <TabsList aria-label={t(nav.email, "Email")}>
            <TabsTrigger value="threads">{t(mail.tab.threads, "Threads")}</TabsTrigger>
            <TabsTrigger value="automatic">{t(mail.tab.automatic, "Automatic replies")}</TabsTrigger>
          </TabsList>
        </Tabs>
      </header>
      {automatic ? (
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-4 py-5">
          <AutoReplies />
        </div>
      ) : (
        <Threads sought={at.params} show={show ?? "needs_you"} />
      )}
    </div>
  );
}

const SHOWS: readonly Show[] = ["needs_you", "waiting", "all"];

const SHOW_NAME: Record<Show, (t: T) => string> = {
  needs_you: (t) => t(mail.show.needs_you, "Needs you"),
  waiting: (t) => t(mail.show.waiting, "Waiting"),
  all: (t) => t(mail.show.all, "All"),
};

const NOTHING: Record<Show, (t: T) => string> = {
  needs_you: (t) => t(mail.nothing.needs_you, "Nothing needs you"),
  waiting: (t) => t(mail.nothing.waiting, "Nothing waits on Google"),
  all: (t) => t(mail.nothing.all, "No mail from Google yet"),
};

const CHIP = "inline-flex min-h-9 shrink-0 items-center gap-2 rounded-full border border-border px-3 text-sm text-ink outline-none focus-visible:ring-2 focus-visible:ring-ring aria-pressed:bg-hover";

/** The view at `show`, a case of it open or none. */
const threads = (show: Show, c?: number) => ({ view: "email", params: { ...(show !== "needs_you" && { show }), ...(c !== undefined && { case: c }) } }) as const;

function Threads({ sought, show }: { sought: Sought; show: Show }) {
  const t = useT();
  const { case: opened } = sought;
  const read = useCases();
  const listed = read.shown === null ? [] : shownIn(read.shown, show);
  // where the opened one stood: marked done, it leaves the list, and its neighbours stay next to it
  const stood = useRef(0);
  const here = listed.findIndex((c) => c.id === opened);
  useEffect(() => {
    if (here !== -1) stood.current = here;
  }, [here]);
  const step = (by: number) => {
    const n = listed.length;
    const from = opened === undefined ? (by > 0 ? 0 : -1) : here !== -1 ? here + by : by > 0 ? stood.current : stood.current - 1;
    const next = listed[(from + n) % n];
    if (next === undefined) throw new Error("a step among no threads");
    go(threads(show, next.id), "replace");
  };
  const other = listed.some((c) => c.id !== opened);
  useBinding({ on: { key: "ArrowDown" }, id: "mail-next", run: { label: t(mail.next, "Next thread"), run: other ? () => step(1) : null } });
  useBinding({ on: { key: "ArrowUp" }, id: "mail-prev", run: { label: t(mail.prev, "Previous thread"), run: other ? () => step(-1) : null } });
  return (
    <div className="flex min-h-0 flex-1">
      <div className={cn("flex min-h-0 min-w-0 flex-1 flex-col @3xl:max-w-80 @3xl:border-r @3xl:border-border @5xl:max-w-110", opened !== undefined && "@max-3xl:hidden")}>
        <div role="group" aria-label={t(mail.show, "Threads listed")} className="flex gap-2 overflow-x-auto border-b border-border px-3 py-2.5">
          {SHOWS.map((s) => (
            <button key={s} type="button" aria-pressed={s === show} className={CHIP} onClick={() => go(threads(s), "replace")}>
              {SHOW_NAME[s](t)}
              {read.shown !== null && <span className="text-ink-soft tabular-nums">{shownIn(read.shown, s).length}</span>}
            </button>
          ))}
        </div>
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <Inboxes />
          {read.status === "error" ? (
            <div className="p-4">
              <ErrorState failure={read.failure} onRetry={read.reload} />
            </div>
          ) : read.shown === null ? (
            <Skeleton className="m-3 h-60" />
          ) : (
            <List cases={listed} read={read.data} all={read.shown.length} show={show} opened={opened} reload={read.reload} />
          )}
        </div>
      </div>
      <div className={cn("flex min-h-0 min-w-0 flex-1 flex-col", opened === undefined && "@max-3xl:hidden")}>
        {opened === undefined ? (
          <p className="px-4 py-16 text-center text-sm text-ink-soft">{t(mail.pick, "Pick a thread to read it here.")}</p>
        ) : (
          <>
            <div className="border-b border-border px-2 py-1.5 @3xl:hidden">
              <Button type="button" variant="ghost" size="touch" onClick={() => go(threads(show), "replace")}>
                <ArrowLeft aria-hidden className="size-5" />
                {t(mail.tab.threads, "Threads")}
              </Button>
            </div>
            <CaseDetail key={opened} id={opened} />
          </>
        )}
      </div>
    </div>
  );
}

/** `read`: the answer `cases` are of. */
function List({ cases, read, all, show, opened, reload }: { cases: readonly MailCase[]; read: unknown; all: number; show: Show; opened: number | undefined; reload: () => void }) {
  const t = useT();
  const gmails = useGmails();
  const box = useRef<HTMLDivElement>(null);
  const picked = useRef<number | null>(null);
  useEffect(() => {
    if (opened !== undefined) box.current?.querySelector(`[data-case="${opened}"]`)?.scrollIntoView({ block: "nearest" });
  }, [opened]);
  // its GBP picked, the picker is gone, and its case may be: the row in its place has the focus
  useEffect(() => {
    if (picked.current === null || box.current === null) return;
    const rows = rowsOf(box.current);
    const row = rows[Math.min(picked.current, rows.length - 1)];
    picked.current = null;
    if (document.activeElement === document.body) row?.focus();
  }, [read]);
  if (cases.length === 0)
    return (
      <EmptyState
        className="m-3 flex-none"
        title={NOTHING[all === 0 ? "all" : show](t)}
        description={all === 0 ? t(mail.nothing.why, "It is read from each gmail's inbox every minute, once its IMAP is set.") : t(mail.nothing.elsewhere, "{n} in All", { n: all })}
      />
    );
  if (gmails.status !== "ok") return <Skeleton className="m-3 h-60" />;
  const { gmails: accounts, gmbs } = gmails.data;
  const of = (c: MailCase) => gmbs.find((g) => g.id === c.gmb_id)?.name ?? accounts.find((g) => g.id === c.gmail_id)?.gmail;
  const waits = show === "needs_you" ? cases.map(stuckOf).filter((why) => why !== null) : [];
  const [why] = waits;
  return (
    <div ref={box} className="flex flex-col gap-2 p-1.5" onKeyDown={arrows}>
      {why !== undefined && (
        <section className="flex flex-col gap-2 rounded-md border border-border p-2.5 text-sm">
          <p className="break-words text-ink">{t(mail.stuck, "{n, plural, one {# thread waits} other {# threads wait}}: {why}", { n: waits.length, why })}</p>
          <Mend />
        </section>
      )}
      <ul className="flex flex-col gap-0.5">
        {cases.map((c, i) => {
          const whose = of(c);
          // its gmail removed, here or in another tab: the read that follows is without it
          if (whose === undefined) return null;
          return (
            <li key={c.id} className="flex flex-col">
              <CaseRow c={c} of={whose} current={c.id === opened} to={threads(show, c.id)} />
              {unnamed(c) && !over(c) && (
                <WhichGbp
                  c={c}
                  onChanged={() => {
                    picked.current = i;
                    reload();
                  }}
                  className="mb-2 ml-2.5 w-60 max-w-[calc(100%-1.25rem)]"
                />
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

const rowsOf = (list: HTMLElement) => [...list.querySelectorAll<HTMLElement>("a[data-case]")];

/** Arrow keys go from row to row. */
function arrows(e: KeyboardEvent<HTMLElement>) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const rows = rowsOf(e.currentTarget);
  const here = rows.indexOf(document.activeElement as HTMLElement);
  if (here === -1) return;
  e.preventDefault();
  rows[here + (e.key === "ArrowDown" ? 1 : -1)]?.focus();
}
