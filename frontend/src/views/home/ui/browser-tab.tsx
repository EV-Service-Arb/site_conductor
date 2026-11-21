"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Planned, cn } from "@evinvest/uikit";
import { Check, Copy, Keyboard, Mail, MonitorSmartphone, ShieldCheck, Smartphone, Video } from "lucide-react";

import { DEVICE_NAME, LANG, type Listed, PROVIDER_NAME, STATE_NAME, googleName, useTrade, windowKeys } from "@/entities/gmb";
import { BrowserSession, type TypeIn } from "@/features/browser-session";
import { api } from "@/shared/api";
import type { Browser, Device } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { go, setHome, useAt } from "@/shared/lib/location";
import { ago } from "@/shared/lib/time";
import { EmptyState } from "@/shared/ui/empty-state";
import { useButtonSize } from "@/shared/ui/touch";
import { launch } from "../model/i18n";
import { OntoGoogle } from "./launch";

/** The GBP's gmail's browser, started at its point, with Google's form beside it step by step (05d). */
export function BrowserTab({ listed }: { listed: Listed }) {
  const t = useT();
  const { gmb, gmail } = listed;
  if (gmail === null) return <EmptyState title={t(windowKeys.browser.needsGmail, "Needs a gmail")} description={t(windowKeys.browser.needsGmail.body, "A GBP's browser is its managing gmail's: pick one on the Launch tab.")} />;
  return (
    <div className="flex flex-col gap-4">
      <BrowserSession
        gmail={gmail}
        gmb={gmb.id}
        aside={(type, browser) => (
          <aside className="flex w-full shrink-0 flex-col gap-4 xl:w-96">
            <GoogleForm listed={listed} type={type} browser={browser} />
            <Sessions gmb={gmb.id} />
          </aside>
        )}
      />
      {gmb.state === "verifying" && <LiveCall />}
    </div>
  );
}

/** Google's form steps, in its order; the operator says where it is, nothing reads the page. */
const STEPS = 8;

/** The step in the URL (?step), so a reload keeps it; past the last, every step done. */
function useStep(): [number, (n: number) => void] {
  const at = useAt();
  const step = at.view === "gbp" || at.view === "home" ? (at.params.step ?? 1) : 1;
  if (step < 1 || step > STEPS + 1) throw new Error(`?step=${step}: Google's form has ${STEPS} steps`);
  const set = (n: number) => (at.view === "gbp" ? go({ view: "gbp", params: { ...at.params, step: n } }, "replace") : setHome({ step: n }));
  return [step, set];
}

interface Step {
  label: string;
  /** What is typed into its field, in order; `null` not set yet. A choice is clicked, not typed: none. */
  typed: readonly string[] | null;
  /** A choice's answer. */
  answer?: string | null;
}

function GoogleForm({ listed: { gmb, gmail }, type, browser }: { listed: Listed; type: TypeIn; browser: Browser }) {
  const t = useT();
  const [step, setStep] = useStep();
  const trade = useTrade(gmb.trade, gmb.country?.code ?? null);
  const category = trade === null || gmb.country === null ? null : googleName(trade, LANG[gmb.country.code]);
  const phone = browser.provider === "multilogin";
  const one = (v: string | null) => (v === null ? null : [v]);
  const steps: Step[] = [
    { label: t(launch.must.name, "Name"), typed: [gmb.name] },
    { label: t(launch.must.category, "Category"), typed: one(category) },
    { label: t(windowKeys.browser.visit, "Customers visit?"), typed: [], answer: gmb.customers_visit === null ? null : gmb.customers_visit ? t(launch.visit.yes, "Customers visit") : t(launch.visit.no, "Customers don't visit") },
    { label: t(windowKeys.browser.areas, "Service areas"), typed: gmb.areas.length === 0 ? null : gmb.areas },
    { label: t(launch.must.phone, "Phone"), typed: one(gmb.phone) },
    { label: t(windowKeys.browser.website, "Website"), typed: one(gmb.website) },
    { label: t(windowKeys.browser.hiddenAddress, "Hidden address"), typed: one(gmb.hidden_address) },
    { label: t(windowKeys.browser.verification, "Verification · video"), typed: [], answer: gmb.film === null ? null : t(windowKeys.browser.film, "film {film}", { film: gmb.film }) },
  ];
  if (steps.length !== STEPS) throw new Error("Google's form is 8 steps");
  const unusable = phone ? t(windowKeys.browser.phoneForm, "Google's form goes in the desktop browser") : type === null ? t(windowKeys.browser.typeOff, "Open the browser first") : null;
  return (
    <section className="flex flex-col gap-3 rounded-lg border border-border p-4">
      <header className="flex flex-col gap-0.5">
        <h3 className="text-sm font-semibold text-ink">{t(windowKeys.browser.formStep, "Google's form · step {n} of {of}", { n: Math.min(step, STEPS), of: STEPS })}</h3>
        <p className="text-xs text-ink-soft">
          {t(windowKeys.browser.whose, "{gmail}'s browser · {provider} · {device} · one browser for every GBP of this gmail", {
            gmail: gmail?.gmail ?? "",
            provider: PROVIDER_NAME[browser.provider](t),
            device: phone ? browser.device : DEVICE_NAME[browser.device as Device](t),
          })}
        </p>
        {phone && (
          <p className="flex items-center gap-1.5 text-xs text-accent-warn">
            <Smartphone aria-hidden className="size-3.5" />
            {t(windowKeys.browser.phone, "A cloud phone is for video submissions and calls: set a desktop browser on the gmail for Google's form.")}
          </p>
        )}
      </header>
      <ol className="flex flex-col">
        {steps.map((s, i) => (
          <StepRow key={s.label} n={i + 1} step={s} at={step} unusable={unusable} type={type} onPick={() => setStep(i + 1)} onDone={() => setStep(i + 2)} />
        ))}
      </ol>
      {step > STEPS - 1 && gmb.state === "draft" && (
        <div className="flex flex-col gap-1">
          <p className="text-xs text-ink-soft">{t(windowKeys.browser.afterNext, "After Next on step 8, it is on Google: verifying.")}</p>
          <OntoGoogle gmbId={gmb.id} onMoved={() => setStep(1)} />
        </div>
      )}
    </section>
  );
}

function StepRow({ n, step, at, unusable, type, onPick, onDone }: { n: number; step: Step; at: number; unusable: string | null; type: TypeIn; onPick: () => void; onDone: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const done = n < at;
  const current = n === at;
  const missing = step.typed === null || step.answer === null;
  return (
    <li className={cn("flex flex-col gap-1.5 border-l-2 py-1.5 pl-3", current ? "border-ink" : "border-transparent")}>
      <button type="button" onClick={onPick} className="flex items-center gap-2 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <span className={cn("flex size-5 shrink-0 items-center justify-center rounded-full font-mono text-xs", done ? "bg-positive/20 text-positive" : current ? "bg-ink text-background" : "bg-hover text-ink-soft")}>{done ? <Check aria-hidden className="size-3" /> : n}</span>
        <span className={cn("flex-1", current ? "font-medium text-ink" : "text-ink-soft")}>{step.label}</span>
        {missing && <span className="text-xs text-accent-warn">{t(windowKeys.browser.unset, "not set")}</span>}
      </button>
      {current && (
        <div className="flex flex-col gap-1.5 pl-7">
          {step.answer !== undefined && step.answer !== null && <span className="text-sm text-ink">{step.answer}</span>}
          {(step.typed ?? []).map((v, i, all) => (
            <span key={v} className="flex items-center gap-1">
              <span className="min-w-0 flex-1 break-words text-sm text-ink">{v}</span>
              <Button
                variant="outline"
                size={button("xs")}
                {...(unusable !== null && { disabledReason: unusable })}
                onClick={() => {
                  if (type === null) throw new Error("typed with no browser open");
                  type(v);
                  if (i === all.length - 1) onDone();
                }}
              >
                <Keyboard aria-hidden />
                {t(windowKeys.browser.type, "Type into the field")}
              </Button>
              <Button variant="ghost" size={button("xs")} icon aria-label={t(windowKeys.browser.copy, "Copy {what}", { what: step.label })} title={t(windowKeys.browser.copy, "Copy {what}", { what: step.label })} onClick={() => void navigator.clipboard.writeText(v)}>
                <Copy aria-hidden />
              </Button>
            </span>
          ))}
          {step.typed !== null && step.typed.length === 0 && (
            <Button variant="outline" size={button("xs")} className="self-start" onClick={onDone}>
              {t(windowKeys.browser.next, "Done · next step")}
            </Button>
          )}
        </div>
      )}
    </li>
  );
}

/** The GBP's browser sessions and what came of them: launches, stops, moves of its state, Google's emails. */
function Sessions({ gmb }: { gmb: number }) {
  const t = useT();
  const locale = useLocale();
  const log = useResource(api, "/gmbs/{id}/log", { params: { path: { id: gmb } } });
  if (log.status !== "ok") return null;
  const rows = log.data.events.flatMap((e) => {
    switch (e.kind) {
      case "launched":
        return [{ id: e.id, at: e.at, icon: <MonitorSmartphone aria-hidden />, text: e.film ? t(windowKeys.log.launchedFilm, "Browser launched, playing its film") : t(windowKeys.log.launched, "Browser launched") }];
      case "stopped":
        return [{ id: e.id, at: e.at, icon: <MonitorSmartphone aria-hidden />, text: t(windowKeys.log.stopped, "Browser stopped") }];
      case "state":
        return [{ id: e.id, at: e.at, icon: <ShieldCheck aria-hidden />, text: `${STATE_NAME[e.from](t)} → ${STATE_NAME[e.to](t)}` }];
      case "email":
        return [{ id: e.id, at: e.at, icon: <Mail aria-hidden />, text: e.subject ?? t(windowKeys.log.noSubject, "(no subject)") }];
      default:
        return [];
    }
  });
  return (
    <section className="flex flex-col gap-2 rounded-lg border border-border p-4">
      <h3 className="text-sm font-semibold text-ink">{t(windowKeys.browser.sessions, "Sessions")}</h3>
      {rows.length === 0 ? (
        <p className="text-xs text-ink-soft">{t(windowKeys.browser.noSessions, "No session yet.")}</p>
      ) : (
        <ol className="flex flex-col gap-1.5">
          {rows.map((r) => (
            <li key={r.id} className="flex items-center gap-2 text-xs [&_svg]:size-3.5 [&_svg]:shrink-0 [&_svg]:text-ink-soft">
              {r.icon}
              <span className="min-w-0 flex-1 truncate text-ink">{r.text}</span>
              <time dateTime={r.at} className="shrink-0 text-ink-soft tabular-nums">
                {ago(r.at, locale)}
              </time>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

/** 02e: a live call on the gmail's cloud phone, its film fed as the camera. */
function LiveCall() {
  const t = useT();
  return (
    <Planned what={t(windowKeys.browser.liveCall, "Live call on the gmail's Multilogin cloud phone beside its desktop browser: a film from its library fed as the call's camera")}>
      <div className="flex gap-4">
        <div className="flex h-80 w-40 shrink-0 items-center justify-center rounded-2xl border border-border bg-black/40">
          <Video aria-hidden className="size-8 text-ink-soft" />
        </div>
        <div className="flex flex-col gap-2 text-sm">
          <p className="font-medium text-ink">{t(windowKeys.browser.films, "Films")}</p>
          <p className="text-ink-soft">verif_vid · 42 · used</p>
          <p className="text-ink-soft">verif_vid · 43 · ready</p>
          <Button size="sm" className="self-start">
            {t(windowKeys.browser.feed, "Feed as the call's camera")}
          </Button>
          <p className="text-xs text-ink-soft">{t(windowKeys.browser.callLog, "Film fed · call joined")}</p>
        </div>
      </div>
    </Planned>
  );
}
