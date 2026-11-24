"use client";

import { useResource } from "@evinvest/api/react";
import { Badge, Button, Card, CardContent, Planned, Skeleton, cn } from "@evinvest/uikit";
import { Check, ExternalLink, Fingerprint, ImageIcon, Mail, ShieldCheck, Sparkles, X } from "lucide-react";

import { type Listed, browser as browserKeys, openWindow, photoSrc, useBrands, useGmbTags, windowKeys } from "@/entities/gmb";
import { startWith } from "@/features/assistant";
import { AutoOf } from "@/features/auto-mail";
import { api } from "@/shared/api";
import { type Gmail, mustHaveValues } from "@/shared/api/schema";
import { type T, useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { failureText } from "@/shared/ui/failure-text";
import { useButtonSize } from "@/shared/ui/touch";

import { MUST_NAME } from "./launch";

const GUIDES = [
  { page: "profile/suspension", name: (t: T) => t(windowKeys.connections.guide.suspension, "Suspension"), about: (t: T) => t(windowKeys.connections.guide.suspension.about, "what links profiles, what to do once off Maps") },
  { page: "profile/verification", name: (t: T) => t(windowKeys.connections.guide.verification, "Verification"), about: (t: T) => t(windowKeys.connections.guide.verification.about, "video, live, instant") },
  { page: "reviews", name: (t: T) => t(windowKeys.connections.guide.reviews, "Reviews"), about: (t: T) => t(windowKeys.connections.guide.reviews.about, "rate, sourcing, deletion") },
  { page: "ranking", name: (t: T) => t(windowKeys.connections.guide.ranking, "Ranking"), about: (t: T) => t(windowKeys.connections.guide.ranking.about, "name, categories, site, what to measure") },
];

const head = "flex items-center gap-2 [&>svg]:size-4 [&>svg]:text-ink-soft [&>h3]:flex-1 [&>h3]:text-sm [&>h3]:font-semibold [&>h3]:text-ink";

/** What it is connected to (02d): its verification kit, its gmail's inbox and browser, its photos. */
export function ConnectionsTab({ listed: { gmb, gmail } }: { listed: Listed }) {
  const t = useT();
  const button = useButtonSize();
  const brands = useBrands();
  const { tags } = useGmbTags();
  const brand = gmb.brand_id === null || brands.status !== "ok" ? undefined : brands.data.find((b) => b.id === gmb.brand_id);
  const site = tags.sites?.get(gmb.id);
  const link = site ? `${site.brand}/${site.location}` : gmb.website;
  const kit = [
    { label: t(windowKeys.connections.kit.phone, "Standardised phone"), value: gmb.phone },
    { label: t(windowKeys.connections.kit.name, "Name"), value: gmb.name },
    { label: t(windowKeys.connections.kit.brandLink, "Brand link"), value: link },
    { label: t(windowKeys.connections.kit.film, "Verification film"), value: gmb.film },
  ];
  const kitReady = kit.filter((k) => k.value !== null).length;
  const missing = gmb.readiness && mustHaveValues.filter((m) => gmb.readiness!.missing.includes(m) || gmb.readiness!.unknown.includes(m));
  return (
    <div className="@container flex flex-col gap-4 p-5">
      {missing !== null && (
        <section className={cn("flex flex-col gap-2 rounded-lg border px-4 py-3", missing.length === 0 ? "border-positive/40 bg-positive/5" : "border-accent-warn/40 bg-accent-warn/5")}>
          <div className="flex items-center gap-3">
            <span aria-hidden className="flex gap-1">
              {mustHaveValues.map((m) => (
                <span key={m} className={cn("h-1.5 w-4 rounded-full", missing.includes(m) ? "bg-ink-soft/40" : "bg-positive")} />
              ))}
            </span>
            <p className="font-medium text-ink">{t(windowKeys.connections.ready, "{n} of {of} ready", { n: mustHaveValues.length - missing.length, of: mustHaveValues.length })}</p>
          </div>
          {missing.length > 0 && <p className="text-sm text-ink-soft">{t(windowKeys.connections.missing, "Missing: {what}", { what: missing.map((m) => MUST_NAME[m](t)).join(" · ") })}</p>}
        </section>
      )}
      <div className="grid grid-cols-1 gap-4 @2xl:grid-cols-2">
        <Card className="gap-3 py-4">
          <CardContent className="flex flex-col gap-3 px-4">
            <header className={head}>
              <ShieldCheck aria-hidden />
              <h3>{t(windowKeys.connections.kit.title, "Verification kit")}</h3>
              <Badge tone={kitReady === kit.length ? "positive" : "warn"}>{t(windowKeys.connections.kit.ready, "{n} of {of}", { n: kitReady, of: kit.length })}</Badge>
            </header>
            <ul className="flex flex-col gap-2 text-sm">
              {kit.map((k) => (
                <li key={k.label} className="flex items-center gap-2">
                  {k.value === null ? <X aria-hidden className="size-4 shrink-0 text-accent-error" /> : <Check aria-hidden className="size-4 shrink-0 text-positive" />}
                  <span className="text-ink">{k.label}</span>
                  <span className="min-w-0 flex-1 truncate text-right font-mono text-xs text-ink-soft">{k.value ?? t(windowKeys.browser.unset, "not set")}</span>
                </li>
              ))}
              {brand && <li className="text-xs text-ink-soft">{t(windowKeys.connections.kit.brand, "Brand: {name}", { name: brand.name })}</li>}
            </ul>
            <Planned what={t(windowKeys.connections.kit.planned, "Directions to the door and the business-card name, which the printables need")}>
              <p className="flex items-center gap-2 text-sm">
                <X aria-hidden className="size-4 text-accent-error" />
                {t(windowKeys.connections.kit.directions, "Directions to the door")}
              </p>
              <p className="flex items-center gap-2 text-sm">
                <Check aria-hidden className="size-4 text-positive" />
                {t(windowKeys.connections.kit.card, "Business-card name")}
                <span className="flex-1 text-right font-mono text-xs text-ink-soft">Aquafix Plomberie</span>
              </p>
            </Planned>
          </CardContent>
        </Card>

        {gmail === null ? (
          <Card className="gap-3 py-4">
            <CardContent className="flex flex-col gap-3 px-4">
              <header className={head}>
                <Mail aria-hidden />
                <h3>{t(windowKeys.connections.inbox.title, "Email · IMAP")}</h3>
              </header>
              <p className="text-sm text-ink-soft">{t(windowKeys.connections.needsGmail, "A GBP's inbox and browser are its managing gmail's: pick one on the Launch tab.")}</p>
            </CardContent>
          </Card>
        ) : (
          <Inbox gmail={gmail} />
        )}

        {gmail !== null && <Browser gmail={gmail.id} address={gmail.gmail} />}

        <Card className="gap-3 py-4">
          <CardContent className="flex flex-col gap-3 px-4">
            <header className={head}>
              <ImageIcon aria-hidden />
              <h3>{t(windowKeys.connections.photos.title, "Photos from our db")}</h3>
            </header>
            {brand ? <BrandPhotos brand={brand.id} gmb={gmb.id} /> : <p className="text-sm text-ink-soft">{t(windowKeys.connections.photos.noBrand, "No brand: its category's photos need one.")}</p>}
            <Planned what={t(windowKeys.connections.photos.planned, "Photos Google removed from this location, and the similar shots it pauses")}>
              <p className="text-sm text-accent-warn">{t(windowKeys.connections.photos.removed, "2 removed by Google this week — similar shots paused")}</p>
            </Planned>
          </CardContent>
        </Card>

        <Card className="gap-3 py-4">
          <CardContent className="flex flex-col gap-3 px-4">
            <header className={head}>
              <Sparkles aria-hidden />
              <h3>{t(windowKeys.connections.playbook.title, "Playbook prompts")}</h3>
            </header>
            <p className="text-sm text-ink-soft">{t(windowKeys.connections.playbook.hint, "The agent answers from these guides when working on this location.")}</p>
            <div className="grid grid-cols-2 gap-2">
              {GUIDES.map((g) => (
                <div key={g.page} className="flex items-center gap-2 rounded-md border border-border px-3 py-2">
                  <span className="flex min-w-0 flex-1 flex-col">
                    <span className="text-sm text-ink">{g.name(t)}</span>
                    <span className="truncate text-xs text-ink-soft" title={g.about(t)}>
                      {g.about(t)}
                    </span>
                  </span>
                  <Button
                    variant="ghost"
                    size={button("sm")}
                    onClick={() => startWith(t(windowKeys.connections.playbook.prompt, "What does the playbook's {page} guide say to do for @{name}?", { page: g.page, name: gmb.name }), { kind: "location", id: gmb.id, label: gmb.name })}
                  >
                    {t(windowKeys.connections.playbook.ask, "Ask")}
                  </Button>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function Inbox({ gmail }: { gmail: Gmail }) {
  const t = useT();
  const locale = useLocale();
  const connections = useResource(api, "/gmails/{id}/connections", { params: { path: { id: gmail.id } } });
  const imap = connections.status === "ok" ? connections.data.imap : null;
  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-3 px-4">
        <header className={head}>
          <Mail aria-hidden />
          <h3>{t(windowKeys.connections.inbox.title, "Email · IMAP")}</h3>
          {imap && <Badge tone={imap.checked.error === null ? "positive" : "error"}>{imap.checked.error === null ? t(windowKeys.connections.inbox.connected, "Connected") : t(browserKeys.failing, "Failing")}</Badge>}
        </header>
        {connections.status === "loading" && <Skeleton className="h-12" />}
        {connections.status === "error" && <p className="text-sm text-accent-error">{failureText(connections.failure, t)}</p>}
        {connections.status === "ok" &&
          (imap === null ? (
            <p className="text-sm text-ink-soft">{t(windowKeys.connections.inbox.none, "{gmail}'s inbox is not read yet: Google's emails need its IMAP.", { gmail: gmail.gmail })}</p>
          ) : (
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-ink-soft">{t(windowKeys.connections.inbox.inbox, "Inbox")}</dt>
              <dd className="truncate text-right text-ink">{imap.username}</dd>
              <dt className="text-ink-soft">{t(windowKeys.connections.inbox.host, "Host")}</dt>
              <dd className="truncate text-right font-mono text-ink">{imap.host}</dd>
              <dt className="text-ink-soft">{t(windowKeys.connections.inbox.checked, "Checked")}</dt>
              <dd className="text-right text-ink-soft">{formatDateTime(imap.checked.at, locale)}</dd>
              {imap.checked.error !== null && <dd className="col-span-2 text-xs text-accent-error">{imap.checked.error}</dd>}
            </dl>
          ))}
        <AutoOf g={gmail} />
      </CardContent>
    </Card>
  );
}

function Browser({ gmail, address }: { gmail: number; address: string }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const connections = useResource(api, "/gmails/{id}/connections", { params: { path: { id: gmail } } });
  const browser = connections.status === "ok" ? connections.data.browser : null;
  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-3 px-4">
        <header className={head}>
          <Fingerprint aria-hidden />
          <h3>{t(windowKeys.connections.browser.title, "Browser · the gmail's")}</h3>
          {browser && <Badge tone={browser.checked.error === null ? "positive" : "error"}>{browser.checked.error === null ? t(browserKeys.healthy, "Healthy") : t(browserKeys.failing, "Failing")}</Badge>}
        </header>
        {connections.status === "loading" && <Skeleton className="h-12" />}
        {connections.status === "error" && <p className="text-sm text-accent-error">{failureText(connections.failure, t)}</p>}
        {connections.status === "ok" &&
          (browser === null ? (
            <p className="text-sm text-ink-soft">{t(windowKeys.connections.browser.none, "{gmail} has no browser yet.", { gmail: address })}</p>
          ) : (
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-ink-soft">{t(browserKeys.exit, "Exit")}</dt>
              <dd className="truncate text-right font-mono text-ink">{[browser.exit.country, browser.exit.city, browser.exit.mobile ? t(browserKeys.mobile, "mobile") : t(browserKeys.fixed, "fixed line"), browser.exit.ip].join(" · ")}</dd>
              <dt className="text-ink-soft">{t(browserKeys.set, "Set")}</dt>
              <dd className="text-right text-ink-soft">{formatDateTime(browser.set_at, locale)}</dd>
              {browser.checked.error !== null && <dd className="col-span-2 text-xs text-accent-error">{browser.checked.error}</dd>}
            </dl>
          ))}
        <p className="text-xs text-ink-soft">{t(windowKeys.connections.browser.why, "Every GBP of this gmail goes out through the one browser: its fingerprint, its proxy, its session.")}</p>
        <div>
          <Button variant="outline" size={button("sm")} onClick={() => openWindow({ kind: "account", gmail, tab: "browser" })}>
            {t(windowKeys.connections.browser.open, "Open {gmail}'s browser", { gmail: address })}
            <ExternalLink aria-hidden />
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

const SHOWN_PHOTOS = 6;

function BrandPhotos({ brand, gmb }: { brand: number; gmb: number }) {
  const t = useT();
  const photos = useResource(api, "/brands/{id}/photos", { params: { path: { id: brand } } });
  const kit = useResource(api, "/gmbs/{id}/kit", { params: { path: { id: gmb }, query: { since: 0 } } });
  return (
    <div className="flex flex-col gap-3 text-sm">
      {photos.status === "loading" && <Skeleton className="h-16" />}
      {photos.status === "error" && <p className="text-accent-error">{failureText(photos.failure, t)}</p>}
      {photos.status === "ok" && (
        <>
          <div className="flex items-baseline justify-between">
            <span className="text-ink-soft">{t(windowKeys.connections.photos.brand, "Its brand's photos")}</span>
            <span className="text-ink tabular-nums">{t(windowKeys.connections.photos.count, "{n} photos", { n: photos.data.photos.length })}</span>
          </div>
          {photos.data.photos.length > 0 && (
            <div className="grid grid-cols-6 gap-1">
              {photos.data.photos.slice(0, SHOWN_PHOTOS).map((p) => (
                <span key={p.id} className="aspect-square overflow-hidden rounded bg-hover">
                  {/* eslint-disable-next-line @next/next/no-img-element -- the API's own JPEG, behind the session: nothing for next/image to optimise in an export */}
                  <img src={photoSrc(brand, p.id)} alt={p.shows ?? ""} loading="lazy" title={p.shows ?? undefined} className="size-full object-cover" />
                </span>
              ))}
            </div>
          )}
        </>
      )}
      {kit.status === "error" && <p className="text-xs text-ink-soft">{failureText(kit.failure, t)}</p>}
      {kit.status === "ok" && (
        <div className="flex items-baseline justify-between">
          <span className="text-ink-soft">{t(windowKeys.connections.photos.kit, "Kit: saved by reviewers")}</span>
          <span className="text-ink tabular-nums">{kit.data.tally.photos_saved}</span>
        </div>
      )}
    </div>
  );
}
