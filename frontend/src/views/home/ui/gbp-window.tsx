"use client";

import { useResource } from "@evinvest/api/react";
import { Badge, Button, NativeSelect, NativeSelectOption, Skeleton, Tabs, TabsList, TabsTrigger, cn, toast } from "@evinvest/uikit";
import { CalendarClock, ChevronLeft, ChevronRight, Copy, ExternalLink, Globe, Hash, History, ImageIcon, Mail, MapPin, MonitorSmartphone, Pencil, Phone, QrCode, RefreshCw, ShieldAlert, ShieldCheck, Star, Store, Tag, UserCog, Video } from "lucide-react";
import { type ReactNode, type RefObject, useEffect, useRef, useState } from "react";

import { CategoryChip, GmbNote, NewBrand, type KeyOf, type Listed, LiveReviews, ROLE_NAME, STATE_NAME, STATE_TONE, closeWindow, gmbPhotoSrc, googleName, TradeName, useGoogleList, narrowed, openWindow, readyCount, under, syncGmb, useBrands, useGmails, useGmbTags, useOnMap, gmbs as gmbsKeys, windowKeys } from "@/entities/gmb";
import { useOpenThread } from "@/entities/mail";
import { useSpendConfirm, useTokens } from "@/entities/tokens";
import { api, ok, reviewArchive } from "@/shared/api";
import { type Changed, type Closed, type Country, type GbpRole, type Gmail, type Gmb, type GmbEdit, type LogDto, type LoggedDto, type Tab, gbpRoleValues, mustHaveValues } from "@/shared/api/schema";
import type { GmbEvent } from "@/shared/api/services/review_archive";
import { type T, useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { ViewLink, useAt } from "@/shared/lib/location";
import { ago } from "@/shared/lib/time";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { useLastWord } from "../model/last-word";
import { AppealTab } from "./appeal-tab";
import { BrowserTab } from "./browser-tab";
import { ConnectionsTab } from "./connections-tab";
import { Launch, TextEditor } from "./launch";
import { RankingTab } from "./ranking-tab";
import { ReviewsTab } from "./reviews-tab";
import { useSplit } from "./split";
import { StatsTab } from "./stats-tab";
import { TAB_ICON, TAB_NAME } from "./tabs";
import { VerificationTab } from "./verification-tab";

type Key = KeyOf<"gbp">;
type Field = Changed["field"];

/** The GBP the window is of, among the caller's; `undefined` while they are read. */
function useListed(gmb: number): Listed | null | undefined {
  const gmails = useGmails();
  if (gmails.status !== "ok") return undefined;
  const listed = narrowed(gmails.data, null, "").find((l) => l.gmb.id === gmb);
  // one added a moment ago postdates a list being read again
  return listed ?? (gmails.fresh ? null : undefined);
}

const CLOSED_WHY = {
  until_verified: (t) => t(windowKeys.closed.untilVerified, "Opens once Google verifies it"),
  frozen_while_verifying: (t) => t(windowKeys.closed.frozen, "Frozen while Google verifies it"),
  off_maps: (t) => t(windowKeys.closed.offMaps, "Off Maps while suspended"),
} satisfies Record<Closed, (t: T) => string>;

/** Its tabs, as its state orders them, the closed greyed with why: the title bar over the map, the strip above a narrow page. */
export function GbpTabs({ k, gmb, except }: { k: Key; gmb: Gmb; except?: Tab }) {
  const t = useT();
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    list.current!.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [k.tab]);
  return (
    <Tabs value={k.tab} onValueChange={(tab) => openWindow({ ...k, tab: tab as Tab })} className="min-w-0 justify-center">
      <TabsList ref={list} aria-label={t(windowKeys.tabs, "Sections")} className="max-w-full justify-start overflow-x-auto">
        {gmb.tabs
          .filter(({ tab }) => tab !== except)
          .map(({ tab, closed }) => (
            <TabsTrigger key={tab} value={tab} className="flex-none" {...(closed !== null && { disabledReason: CLOSED_WHY[closed](t) })}>
              {TAB_ICON[tab]}
              {TAB_NAME[tab](t)}
              <TabBadge tab={tab} gmb={gmb} />
            </TabsTrigger>
          ))}
      </TabsList>
    </Tabs>
  );
}

/** Reviews: how many are suspended; Connections: a draft's must-haves ready. */
function TabBadge({ tab, gmb }: { tab: Tab; gmb: Gmb }) {
  const t = useT();
  const { tags } = useGmbTags();
  const suspended = tags.archive?.get(gmb.id)?.suspended ?? 0;
  if (tab === "reviews" && suspended > 0)
    return (
      <Badge tone="error" title={t(gmbsKeys.tag.suspended, "{n} reviews suspended", { n: suspended })}>
        {suspended}
      </Badge>
    );
  if (tab === "connections" && gmb.readiness !== null) return <Badge tone="neutral">{`${readyCount(gmb.readiness)}/${mustHaveValues.length}`}</Badge>;
  return null;
}

export function GbpTitle({ k, page }: { k: Key; page: boolean }) {
  const t = useT();
  const listed = useListed(k.gmb);
  if (listed === undefined || listed === null) return <span />;
  if (!page) return <GbpTabs k={k} gmb={listed.gmb} />;
  return (
    <span className="flex min-w-0 items-center gap-2">
      <h2 className="truncate font-semibold text-ink">{listed.gmb.name}</h2>
      <Badge tone={STATE_TONE[listed.gmb.state]}>{STATE_NAME[listed.gmb.state](t)}</Badge>
    </span>
  );
}

/** A GBP's window: Google's profile as Sync pulled it and what the conductor holds of it, its log, and its state's own tabs; a page wide enough splits Profile beside the tab open (15c). */
export function GbpBody({ k, page }: { k: Key; page: boolean }) {
  const listed = useListed(k.gmb);
  const [setBox, wide] = useSplit(page);
  const tabs = listed?.gmb.tabs;
  // removed, here or in another tab
  useEffect(() => {
    if (listed === null) closeWindow(k);
  }, [listed, k]);
  // its state moved since the window opened, or the tab is closed in it: its first open one
  const first = tabs?.find((x) => x.closed === null)?.tab;
  const lost = tabs !== undefined && !tabs.some((x) => x.tab === k.tab && x.closed === null);
  useEffect(() => {
    if (lost && first !== undefined) openWindow({ ...k, tab: first });
  }, [lost, first, k]);
  if (listed === undefined) return <Skeleton className="m-4 h-96" />;
  if (listed === null || lost) return null;
  return (
    <div ref={setBox} className="flex min-h-0 flex-1 flex-col">
      <LocationHeader k={k} listed={listed} page={page} />
      {wide ? <Split k={k} listed={listed} page={page} /> : <Tabbed k={k} listed={listed} page={page} />}
    </div>
  );
}

function Split({ k, listed, page }: { k: Key; listed: Listed; page: boolean }) {
  const other = k.tab === "profile" ? "log" : k.tab;
  return (
    <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,36rem)_minmax(0,1fr)] divide-x divide-border">
      <section className="flex min-h-0 flex-col overflow-y-auto">
        <h3 className="flex items-center gap-2 border-b border-border px-4 py-2 text-sm font-semibold text-ink">
          {TAB_ICON.profile}
          <TabLabel tab="profile" />
        </h3>
        <Profile listed={listed} />
      </section>
      <section className="flex min-h-0 flex-col overflow-y-auto">
        <div className="flex h-10 border-b border-border">
          <GbpTabs k={{ ...k, tab: other }} gmb={listed.gmb} except="profile" />
        </div>
        <TabBody tab={other} listed={listed} page={page} />
      </section>
    </div>
  );
}

function Tabbed({ k, listed, page }: { k: Key; listed: Listed; page: boolean }) {
  return (
    <div className="flex min-h-0 flex-col">
      {page && (
        <div className="flex h-10 shrink-0 overflow-x-auto border-b border-border">
          <GbpTabs k={k} gmb={listed.gmb} />
        </div>
      )}
      <TabBody tab={k.tab} listed={listed} page={page} />
    </div>
  );
}

function TabLabel({ tab }: { tab: Tab }) {
  const t = useT();
  return <>{TAB_NAME[tab](t)}</>;
}

function TabBody({ tab, listed, page }: { tab: Tab; listed: Listed; page: boolean }) {
  switch (tab) {
    case "profile":
      return <Profile listed={listed} />;
    case "log":
      return <Log gmb={listed.gmb.id} gmail={listed.gmail?.gmail ?? null} />;
    case "launch":
      return (
        <div className="p-6">
          <Launch listed={listed} page={page} />
        </div>
      );
    case "browser":
      return (
        <div className="p-6">
          <BrowserTab listed={listed} />
        </div>
      );
    case "connections":
      return <ConnectionsTab listed={listed} />;
    case "stats":
      return <StatsTab listed={listed} />;
    case "ranking":
      return <RankingTab listed={listed} />;
    case "reviews":
      return <ReviewsTab listed={listed} />;
    case "verification":
      return <VerificationTab listed={listed} />;
    case "appeal":
      return <AppealTab listed={listed} />;
  }
}

/** What every tab sits under (02): who it is, how it stands, where it is; the GBPs in the map's view a step away. */
function LocationHeader({ k, listed: { gmb, gmail }, page }: { k: Key; listed: Listed; page: boolean }) {
  const t = useT();
  const { tags } = useGmbTags();
  const archive = tags.archive?.get(gmb.id);
  const site = tags.sites?.get(gmb.id);
  const dot = <span aria-hidden>·</span>;
  return (
    <header className="flex shrink-0 flex-col gap-1 border-b border-border px-5 py-3">
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        {/* as a page, its title's */}
        {!page && (
          <>
            <h2 className="min-w-0 truncate text-lg font-semibold text-ink">{gmb.name}</h2>
            <Badge tone={STATE_TONE[gmb.state]}>{STATE_NAME[gmb.state](t)}</Badge>
          </>
        )}
        <CategoryChip gmb={gmb} />
        {archive?.rating != null && (
          <span className="flex items-center gap-1 text-sm text-ink tabular-nums">
            <Star aria-hidden className="size-3.5 fill-current" />
            {archive.rating.toFixed(1)}
            {archive.reviews !== null && <span className="text-ink-soft">({archive.reviews})</span>}
          </span>
        )}
        {archive && <LiveReviews days={archive.live_30d} />}
        {/* those tabs lead with it */}
        {k.tab !== "verification" && k.tab !== "appeal" && <StateNote gmb={gmb.id} />}
        <span className="flex-1" />
        {gmb.place_id !== null && (
          <a href={`https://www.google.com/maps/place/?q=place_id:${gmb.place_id}`} target="_blank" rel="noreferrer" className="flex items-center gap-1 text-xs text-ink-soft hover:text-ink">
            {t(windowKeys.header.maps, "Maps")}
            <ExternalLink aria-hidden className="size-3" />
          </a>
        )}
        <InViewPager k={k} />
      </div>
      <p className="flex min-w-0 flex-wrap items-center gap-x-2 text-xs text-ink-soft">
        {[
          gmb.address,
          gmb.phone,
          gmb.website !== null ? <Website url={gmb.website} /> : site && t(windowKeys.header.site, "its site"),
          gmb.place_id && <span className="font-mono">{gmb.place_id}</span>,
          gmail?.gmail,
        ]
          .filter(Boolean)
          .flatMap((part, i) => (i === 0 ? [<span key={i}>{part}</span>] : [<span key={`d${i}`}>{dot}</span>, <span key={i}>{part}</span>]))}
      </p>
      <GmbNote key={gmb.id} gmb={gmb.id} />
    </header>
  );
}

/** Its site as written, a top-level domain other than .com marked: those rank weaker. */
function Website({ url }: { url: string }) {
  const t = useT();
  const host = /^(?:[a-z][a-z0-9+.-]*:\/\/)?([^/:?#\s]+)/i.exec(url)?.[1];
  const dot = host?.lastIndexOf(".") ?? -1;
  // free text, not a URL: nothing to mark
  if (host === undefined || dot < 0 || host.slice(dot + 1).toLowerCase() === "com") return <>{url}</>;
  const from = url.indexOf(host) + dot;
  const to = from + host.length - dot;
  return (
    <>
      {url.slice(0, from)}
      <span className="text-accent-warn" title={t(windowKeys.header.notCom, "Not .com: other domains rank weaker")}>
        {url.slice(from, to)}
      </span>
      {url.slice(to)}
    </>
  );
}

/** The latest word on its standing: a move of its state, or Google's mail. */
function StateNote({ gmb }: { gmb: number }) {
  const t = useT();
  const locale = useLocale();
  const word = useLastWord(gmb);
  if (word == null) return null;
  return (
    <span className="min-w-0 truncate text-xs text-ink-soft">
      {"who" in word
        ? t(windowKeys.header.email, "Google: {said} · {ago}", { said: word.summary ?? word.subject ?? t(windowKeys.log.noSubject, "(no subject)"), ago: ago(word.at, locale) })
        : t(windowKeys.header.moved, "{state} {ago}", { state: STATE_NAME[word.to](t), ago: ago(word.at, locale) })}
    </span>
  );
}

/** The GBPs in the map's view, in the outline's order: the one before and after this. */
function InViewPager({ k }: { k: Key }) {
  const t = useT();
  const inView = useOnMap((s) => s.inView);
  const gmails = useGmails();
  if (inView === null || gmails.status !== "ok") return null;
  const ids = narrowed(gmails.data, null, "")
    .map((l) => l.gmb.id)
    .filter((id) => inView.has(id));
  const at = ids.indexOf(k.gmb);
  if (at === -1 || ids.length < 2) return null;
  const go = (step: number) => openWindow({ ...k, gmb: ids[(at + step + ids.length) % ids.length]! });
  return (
    <span className="flex items-center text-xs text-ink-soft tabular-nums">
      <Button variant="ghost" size="xs" icon aria-label={t(windowKeys.header.previous, "Previous in view")} onClick={() => go(-1)}>
        <ChevronLeft aria-hidden />
      </Button>
      {t(windowKeys.header.inView, "{at}/{n} in view", { at: at + 1, n: ids.length })}
      <Button variant="ghost" size="xs" icon aria-label={t(windowKeys.header.next, "Next in view")} onClick={() => go(1)}>
        <ChevronRight aria-hidden />
      </Button>
    </span>
  );
}

const SHOWN_PHOTOS = 5;

/** What Google shows of it as its last Sync pulled it, then what Google does not hold (15a). */
function Profile({ listed }: { listed: Listed }) {
  const t = useT();
  const locale = useLocale();
  const { gmb, gmail } = listed;
  const { tags } = useGmbTags();
  const site = tags.sites?.get(gmb.id);
  const primary = gmb.country !== null && gmb.google_primary !== null ? <GooglePrimary country={gmb.country.code} id={gmb.google_primary} /> : null;
  const photos = gmb.photos;
  const more = photos.length - SHOWN_PHOTOS;
  return (
    <div className="flex flex-col">
      {photos.length > 0 ? (
        <div className="grid h-44 grid-cols-4 grid-rows-2 gap-1">
          {photos.slice(0, SHOWN_PHOTOS).map((p, i) => (
            <span key={p.id} className={cn("relative overflow-hidden bg-hover", i === 0 && "col-span-2 row-span-2")}>
              {/* eslint-disable-next-line @next/next/no-img-element -- the API's own JPEG, behind the session: nothing for next/image to optimise in an export */}
              <img src={gmbPhotoSrc(gmb.id, p.id)} alt="" loading="lazy" title={p.author ?? undefined} className="size-full object-cover" />
              {i === SHOWN_PHOTOS - 1 && more > 0 && <span className="absolute right-1.5 bottom-1.5 rounded bg-background/80 px-1.5 text-xs text-ink">+{more}</span>}
            </span>
          ))}
        </div>
      ) : (
        <div className="flex h-24 items-center justify-center gap-2 bg-hover/50 text-sm text-ink-soft">
          <ImageIcon aria-hidden className="size-4" />
          {gmb.state === "verified" ? t(windowKeys.profile.noPhotos, "No photos: Sync pulls Google's") : t(windowKeys.profile.offGoogle, "Not on Maps: no photos from Google")}
        </div>
      )}
      {primary !== null && (
        <p className="flex flex-wrap gap-x-2 px-5 pt-3 text-sm text-ink-soft">
          {primary}
          {gmb.trade !== null && gmb.trade !== gmb.google_primary && (
            <span className="text-accent-warn">
              {t(windowKeys.profile.tradeApart, "on Google; its trade here is")} <TradeName gcid={gmb.trade} country={gmb.country?.code ?? null} />
            </span>
          )}
        </p>
      )}
      <Actions listed={listed} />
      <dl className="flex flex-col gap-3 border-t border-border px-5 py-4 text-sm">
        <Fact icon={<MapPin aria-hidden />} value={gmb.address ?? (gmb.location && t(windowKeys.planned, "planned · {at}", { at: `${gmb.location.lat.toFixed(3)}, ${gmb.location.lng.toFixed(3)}` }))} note={[gmb.customers_visit === null ? null : gmb.customers_visit ? t(windowKeys.profile.visit, "customers visit") : t(windowKeys.profile.serviceArea, "service area"), gmb.location && (gmb.location.placed ? t(windowKeys.profile.pinByHand, "pin placed by hand") : t(windowKeys.profile.pinGoogle, "pin from Google"))]} />
        {gmb.hours.length > 0 && (
          <Fact
            icon={<CalendarClock aria-hidden />}
            value={
              <details>
                <summary className="cursor-pointer">{t(windowKeys.profile.hours, "Hours")}</summary>
                {gmb.hours.map((h) => (
                  <p key={h} className="text-ink-soft">
                    {h}
                  </p>
                ))}
              </details>
            }
          />
        )}
        <Fact icon={<Phone aria-hidden />} value={gmb.phone && <a href={`tel:${gmb.phone}`}>{gmb.phone}</a>} />
        <Fact
          icon={<Globe aria-hidden />}
          value={gmb.website && <Website url={gmb.website} />}
          note={[site && t(windowKeys.profile.site, "its site · {n} leads in 7 days", { n: site.leads_7d })]}
          action={gmb.website !== null && /^https?:\/\//.test(gmb.website) && <a href={gmb.website} target="_blank" rel="noreferrer" className="flex items-center gap-1 text-xs text-ink-soft hover:text-ink">{t(windowKeys.profile.openSite, "Open")}<ExternalLink aria-hidden className="size-3" /></a>}
        />
        <Fact icon={<Hash aria-hidden />} value={gmb.place_id && <span className="font-mono text-xs">{gmb.place_id}</span>} action={gmb.place_id && <CopyButton text={gmb.place_id} />} />
      </dl>
      <dl className="flex flex-col gap-3 border-t border-border px-5 py-4 text-sm">
        <dt className="font-mono text-xs text-ink-soft uppercase">
          {t(windowKeys.profile.ours, "Ours")} <span className="normal-case text-ink-faint">{t(windowKeys.profile.ours.what, "what Google does not hold")}</span>
        </dt>
        <Fact icon={<Mail aria-hidden />} value={gmail?.gmail ?? t(windowKeys.profile.noGmail, "No gmail yet")} action={gmail && <Go label={t(windowKeys.profile.connections, "Connections")} onClick={() => openWindow({ kind: "account", gmail: gmail.id, tab: "connections" })} />} />
        {gmail && <Fact icon={<UserCog aria-hidden />} value={<RoleEditor gmb={gmb} gmail={gmail} />} />}
        <Fact icon={<Tag aria-hidden />} value={<CategoryChip gmb={gmb} />} />
        {gmail && gmb.trade !== null && gmb.country !== null && (
          <Fact
            icon={<Store aria-hidden />}
            value={<BrandEditor gmb={gmb} gmail={gmail.id} trade={gmb.trade} country={gmb.country.code} />}
            action={gmb.brand_id !== null && <Go label={t(windowKeys.profile.brand, "Brand")} onClick={() => openWindow({ kind: "brand", brand: gmb.brand_id! })} />}
          />
        )}
        <Fact icon={<History aria-hidden />} value={t(windowKeys.profile.added, "Added {at}", { at: formatDateTime(gmb.created_at, locale) })} note={[gmb.synced_at !== null && t(windowKeys.profile.synced, "synced from Google {ago}", { ago: ago(gmb.synced_at, locale) })]} />
      </dl>
    </div>
  );
}

/** Its brand: one of its gmail's in its category, or none; a new one is made under both. */
function BrandEditor({ gmb, gmail, trade, country }: { gmb: Gmb; gmail: number; trade: string; country: Country }) {
  const t = useT();
  const gmails = useGmails();
  const brands = useBrands();
  const [adding, setAdding] = useState(false);
  const fits = brands.status === "ok" ? brands.data.filter((b) => b.gmail_id === gmail && b.trade === trade && b.country === country) : [];
  const give = async (brand: number | null) => {
    try {
      await ok(api.PUT("/gmbs/brand", { body: { gmbs: [gmb.id], brand } }));
      gmails.reload();
      brands.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  if (adding)
    return (
      <NewBrand
        gmail={gmail}
        trade={trade}
        country={country}
        onDone={(added) => {
          setAdding(false);
          if (added !== null) void give(added.id);
        }}
      />
    );
  return (
    <NativeSelect
      size="sm"
      wrapperClassName="w-64"
      aria-label={t(windowKeys.profile.brand, "Brand")}
      value={gmb.brand_id ?? ""}
      onChange={(e) => (e.target.value === "new" ? setAdding(true) : void give(e.target.value === "" ? null : Number(e.target.value)))}
    >
      <NativeSelectOption value="">{t(windowKeys.profile.noBrand, "No brand: no logo, no kit photos")}</NativeSelectOption>
      {fits.map((b) => (
        <NativeSelectOption key={b.id} value={b.id}>
          {b.name}
        </NativeSelectOption>
      ))}
      <NativeSelectOption value="new">{t(windowKeys.profile.newBrand, "New brand…")}</NativeSelectOption>
    </NativeSelect>
  );
}

/** Its gmail's role on it; a manager's owner gmail beside, for reference. A gmail usually owns up to 3 GBPs and manages up to 6. */
function RoleEditor({ gmb, gmail }: { gmb: Gmb; gmail: Gmail }) {
  const t = useT();
  const gmails = useGmails();
  const save = async (edit: GmbEdit) => {
    try {
      await ok(api.PATCH("/gmbs/{id}", { params: { path: { id: gmb.id } }, body: edit }));
      gmails.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  const held = gmails.status === "ok" ? under(gmails.data, gmail.id) : [];
  const count = (r: GbpRole) => held.filter((b) => b.role === r).length;
  return (
    <span className="flex flex-col gap-2">
      <NativeSelect
        size="sm"
        wrapperClassName="w-32"
        aria-label={t(windowKeys.profile.role, "Role of {gmail} on it", { gmail: gmail.gmail })}
        title={t(windowKeys.profile.role.hint, "{gmail} owns {owned} of usually up to 3 GBPs, manages {managed} of 6", { gmail: gmail.gmail, owned: count("owner"), managed: count("manager") })}
        value={gmb.role ?? ""}
        onChange={(e) => void save({ role: e.target.value === "" ? null : (e.target.value as GbpRole) })}
      >
        <NativeSelectOption value="">{t(windowKeys.profile.role.pick, "No role")}</NativeSelectOption>
        {gbpRoleValues.map((r) => (
          <NativeSelectOption key={r} value={r}>
            {ROLE_NAME[r](t)}
          </NativeSelectOption>
        ))}
      </NativeSelect>
      {gmb.role === "manager" && <TextEditor key={gmb.owner_gmail} value={gmb.owner_gmail} type="email" label={t(windowKeys.profile.ownerGmail, "Owner gmail")} onSave={(v) => save({ owner_gmail: v })} />}
    </span>
  );
}

function Fact({ icon, value, note = [], action }: { icon: ReactNode; value: ReactNode; note?: readonly (string | false | null | undefined)[]; action?: ReactNode }) {
  if (value === null || value === undefined || value === "") return null;
  const said = note.filter(Boolean);
  return (
    <div className="flex items-start gap-3 [&_svg]:size-4">
      <dt className="mt-0.5 shrink-0 text-ink-soft">{icon}</dt>
      <dd className="flex min-w-0 flex-1 flex-col gap-0.5 text-ink">
        <span className="min-w-0 break-words">{value}</span>
        {said.length > 0 && <span className="text-xs text-ink-soft">{said.join(" · ")}</span>}
      </dd>
      {action}
    </div>
  );
}

function Go({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} className="shrink-0 rounded text-xs text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
      {label} →
    </button>
  );
}

function CopyButton({ text }: { text: string }) {
  const t = useT();
  const button = useButtonSize();
  return (
    <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.profile.copy, "Copy")} title={t(windowKeys.profile.copy, "Copy")} onClick={() => void navigator.clipboard.writeText(text).then(() => toast(t(windowKeys.profile.copied, "Copied")))}>
      <Copy aria-hidden />
    </Button>
  );
}

/** Google's primary category in the reader's language, once the country's list is read. */
function GooglePrimary({ country, id }: { country: Country; id: string }) {
  const locale = useLocale();
  const list = useGoogleList(country);
  if (list.status !== "ok") return null;
  const g = list.data.find((c) => c.id === id);
  if (g === undefined) throw new Error(`${id} is not among ${country}'s Google categories`);
  return <>{googleName(g, locale)}</>;
}

/** Google's row of buttons, as the conductor's: the archive's reviews, its browser, Sync, its review kit, Maps. */
function Actions({ listed: { gmb, gmail } }: { listed: Listed }) {
  const t = useT();
  const locale = useLocale();
  const { tags } = useGmbTags();
  const archive = tags.archive?.get(gmb.id);
  const held = useTokens();
  const spend = useSpendConfirm();
  const { reload } = useGmails();
  const prices = held.status === "ok" ? held.data.prices : null;
  const [profilePrice, photoPrice] = [prices?.["places.profile"], prices?.["places.photo"]];
  // up to 10 photos: Google says how many only once asked
  const syncCost = profilePrice === undefined || photoPrice === undefined ? undefined : profilePrice + 10 * photoPrice;
  const [syncing, setSyncing] = useState(false);
  const sync = () =>
    void spend.ask(syncCost, async () => {
      setSyncing(true);
      try {
        const synced = await syncGmb(gmb);
        toast(t(gmbsKeys.sync.done, "{name} synced from Google", { name: synced.name }));
      } catch (e) {
        notifyFailure(e, t);
      } finally {
        setSyncing(false);
      }
      reload();
    });
  const item = "flex w-20 flex-col items-center gap-1 rounded-md py-1 text-center text-xs text-ink outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 disabled:hover:bg-transparent";
  const disc = "flex size-10 items-center justify-center rounded-full border border-border [&_svg]:size-4";
  const sub = "max-w-full truncate text-xs text-ink-soft";
  return (
    <div className="flex flex-wrap justify-around gap-1 px-3 py-3">
      {spend.dialog}
      <ViewLink to={{ view: "review_archive.mine.gmb", params: { gmb: gmb.id } }} className={item}>
        <span className={disc}>
          <Star aria-hidden />
        </span>
        {t(windowKeys.profile.reviews, "Reviews")}
        {archive && archive.suspended > 0 && <span className={sub}>{t(gmbsKeys.tag.suspended, "{n} reviews suspended", { n: archive.suspended })}</span>}
      </ViewLink>
      {gmb.tabs.some((x) => x.tab === "browser") && (
        <button type="button" className={item} onClick={() => openWindow({ kind: "gbp", gmb: gmb.id, tab: "browser" })}>
          <span className={disc}>
            <MonitorSmartphone aria-hidden />
          </span>
          {t(windowKeys.tab.browser, "Browser")}
          {gmail && <span className={sub}>{gmail.gmail}</span>}
        </button>
      )}
      {gmb.state === "verified" && (
        <button type="button" className={item} disabled={syncing} onClick={sync} title={syncCost === undefined ? undefined : t(windowKeys.profile.syncCost, "up to {n} tokens", { n: syncCost })}>
          <span className={disc}>
            <RefreshCw aria-hidden className={cn(syncing && "animate-spin")} />
          </span>
          {t(windowKeys.profile.sync, "Sync")}
          {gmb.synced_at !== null && <span className={sub}>{ago(gmb.synced_at, locale)}</span>}
        </button>
      )}
      <KitsOf gmb={gmb.id} className={item} disc={disc} sub={sub} />
      {gmb.place_id !== null && (
        <a href={`https://www.google.com/maps/place/?q=place_id:${gmb.place_id}`} target="_blank" rel="noreferrer" className={item}>
          <span className={disc}>
            <ExternalLink aria-hidden />
          </span>
          {t(windowKeys.profile.onGoogle, "On Google")}
        </a>
      )}
    </div>
  );
}

/** A button for each review kit the GBP is in. */
function KitsOf({ gmb, ...look }: { gmb: number; className: string; disc: string; sub: string }) {
  const kits = useResource(api, "/kits");
  if (kits.status !== "ok") return null;
  return kits.data.kits.map((k) => <KitOf key={k.slug} slug={k.slug} name={k.name} gmb={gmb} {...look} />);
}

function KitOf({ slug, name, gmb, className, disc, sub }: { slug: string; name: string; gmb: number; className: string; disc: string; sub: string }) {
  const t = useT();
  const kit = useResource(api, "/kits/{slug}", { params: { path: { slug }, query: { since: 0 } } }); // its members only: any period
  if (kit.status !== "ok" || !kit.data.gmbs.some((g) => g.id === gmb)) return null;
  return (
    <button type="button" className={className} onClick={() => openWindow({ kind: "kit", at: { kit: slug, gmb } })}>
      <span className={disc}>
        <QrCode aria-hidden />
      </span>
      {t(windowKeys.profile.kit, "Review kit")}
      <span className={sub}>{name}</span>
    </button>
  );
}

/** The log's chips: what an event is about. */
type Topic = "reviews" | "verification" | "emails" | "edits" | "syncs";
const TOPICS: readonly Topic[] = ["reviews", "verification", "emails", "edits", "syncs"];
const TOPIC_NAME = {
  reviews: (t) => t(windowKeys.log.reviews, "Reviews"),
  verification: (t) => t(windowKeys.log.verification, "Verification"),
  emails: (t) => t(windowKeys.log.emails, "Emails"),
  edits: (t) => t(windowKeys.log.edits, "Edits"),
  syncs: (t) => t(windowKeys.log.syncs, "Syncs"),
} satisfies Record<Topic, (t: T) => string>;

/** One line of the log, from either source: the conductor's own, or the archive's. */
type Line = { at: string; topic: Topic; key: string; ours: LoggedDto } | { at: string; topic: Topic; key: string; archive: GmbEvent };

function topicOfOurs(e: LoggedDto): Topic {
  switch (e.kind) {
    case "synced":
      return "syncs";
    case "state":
    case "launched":
    case "stopped":
    case "film":
      return "verification";
    case "email":
      return "emails";
    case "added":
    case "edited":
    case "gmail":
    case "trade":
    case "brand":
    case "switched":
    case "removed":
    case "restored":
      return "edits";
  }
}

/** A source's pages, newest first: the first read live, older ones as asked. */
function usePaged<P extends { next: unknown }>(first: P | null, more: (next: NonNullable<P["next"]>) => Promise<P>) {
  const [older, setOlder] = useState<P[]>([]);
  const last = older.at(-1) ?? first;
  const next = last === null ? null : (last.next as P["next"] | null);
  return {
    pages: first === null ? [] : [first, ...older],
    next: next ?? null,
    more: async () => {
      if (next === null || next === undefined) throw new Error("no older page");
      setOlder([...older, await more(next as NonNullable<P["next"]>)]);
    },
  };
}

/**
 * What happened to the GBP (15b): the conductor's log and review_archive's events, merged by time on the page, each source paged
 * back on its own; shown down to where both are read, so neither leaves a gap.
 */
function Log({ gmb, gmail }: { gmb: number; gmail: string | null }) {
  const t = useT();
  const locale = useLocale();
  const ours = useResource(api, "/gmbs/{id}/log", { params: { path: { id: gmb } } });
  const theirs = useResource(reviewArchive, "/me/gmbs/{gmb}/events", { params: { path: { gmb } } });
  const oursPaged = usePaged<LogDto>(ours.status === "ok" ? ours.data : null, (before) => ok(api.GET("/gmbs/{id}/log", { params: { path: { id: gmb }, query: { before } } })));
  // a GBP the archive was never told of (a draft, one with no place) has no events there
  const archiveKnows = !(theirs.status === "error" && theirs.failure.code === "not_found");
  const theirsPaged = usePaged(theirs.status === "ok" ? theirs.data : null, (before: string) => ok(reviewArchive.GET("/me/gmbs/{gmb}/events", { params: { path: { gmb }, query: { before } } })));
  const [topic, setTopic] = useState<Topic | null>(null);
  const [busy, setBusy] = useState(false);
  const at = useAt();
  const focus = at.view === "gbp" || at.view === "home" ? (at.params.log_at ?? null) : null;
  const box = useRef<HTMLDivElement>(null);
  if (ours.status === "loading" || (archiveKnows && theirs.status === "loading")) return <Skeleton className="m-4 h-64" />;
  if (ours.status === "error")
    return (
      <div className="p-4">
        <ErrorState failure={ours.failure} onRetry={ours.reload} />
      </div>
    );
  if (archiveKnows && theirs.status === "error")
    return (
      <div className="p-4">
        <ErrorState failure={theirs.failure} onRetry={theirs.reload} />
      </div>
    );
  const lines: Line[] = [
    ...oursPaged.pages.flatMap((p) => p.events.map((e): Line => ({ at: e.at, topic: topicOfOurs(e), key: `c${e.id}`, ours: e }))),
    ...theirsPaged.pages.flatMap((p) => p.events.map((e, i): Line => ({ at: e.at, topic: "reviews", key: `a${e.at}${i}${e.kind}`, archive: e }))),
  ].sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
  // what a source with older pages has not read yet may sit above another's oldest
  const horizons = [
    oursPaged.next !== null && oursPaged.pages.flatMap((p) => p.events).at(-1)?.at,
    archiveKnows && theirsPaged.next !== null && theirsPaged.pages.flatMap((p) => p.events).at(-1)?.at,
  ].filter((h): h is string => typeof h === "string");
  const cut = horizons.length === 0 ? null : Math.max(...horizons.map(Date.parse));
  const read = cut === null ? lines : lines.filter((l) => Date.parse(l.at) >= cut);
  const shown = topic === null ? read : read.filter((l) => l.topic === topic);
  const days = new Map<string, Line[]>();
  for (const l of shown) {
    const day = new Date(l.at).toLocaleDateString(locale, { day: "numeric", month: "short", year: "numeric" });
    days.set(day, [...(days.get(day) ?? []), l]);
  }
  const older = oursPaged.next !== null || (archiveKnows && theirsPaged.next !== null);
  const more = async () => {
    setBusy(true);
    try {
      // the source whose horizon is the cut reads back first
      await Promise.all([oursPaged.next !== null ? oursPaged.more() : null, archiveKnows && theirsPaged.next !== null ? theirsPaged.more() : null]);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  const chip = (on: boolean) => cn("flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs whitespace-nowrap outline-none focus-visible:ring-2 focus-visible:ring-ring", on ? "border-ink bg-ink text-background" : "border-border text-ink hover:bg-hover");
  // newest first: the first line at or before it is the one it names
  const target = focus === null ? undefined : shown.find((l) => Date.parse(l.at) <= Date.parse(focus))?.key;
  return (
    <div ref={box} className="flex flex-col">
      {focus !== null && <Reach key={focus} box={box} target={target ?? null} more={target === undefined && older && !busy ? more : null} />}
      <div className="flex gap-1.5 overflow-x-auto border-b border-border px-4 py-3">
        <button type="button" className={chip(topic === null)} onClick={() => setTopic(null)}>
          {t(windowKeys.log.all, "All")} <span className="tabular-nums opacity-70">{read.length}</span>
        </button>
        {TOPICS.map((k) => (
          <button key={k} type="button" className={chip(topic === k)} onClick={() => setTopic(k)}>
            {TOPIC_NAME[k](t)} <span className="tabular-nums opacity-70">{read.filter((l) => l.topic === k).length}</span>
          </button>
        ))}
      </div>
      {shown.length === 0 && <p className="px-5 py-6 text-sm text-ink-soft">{topic === "emails" ? t(windowKeys.log.noEmails, "No emails with Google: they are read from the gmail's inbox once its IMAP is set.") : t(windowKeys.log.none, "Nothing logged yet.")}</p>}
      {[...days].map(([day, ls]) => (
        <section key={day} className="flex flex-col px-4 pt-4">
          <h4 className="pb-2 font-mono text-xs text-ink-soft uppercase">{day}</h4>
          <ol className="flex flex-col gap-3">
            {ls.map((l) => (
              <li key={l.key} data-line={l.key} className={cn("flex items-start gap-3", l.key === target && "-mx-2 rounded-md bg-hover px-2 py-1")}>
                <time dateTime={l.at} className="w-10 shrink-0 pt-0.5 font-mono text-xs text-ink-soft tabular-nums">
                  {new Date(l.at).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })}
                </time>
                {"ours" in l ? <Ours e={l.ours} gmail={gmail} /> : <Archive e={l.archive} gmb={gmb} />}
              </li>
            ))}
          </ol>
        </section>
      ))}
      {older && (
        <button type="button" disabled={busy} onClick={() => void more()} className="py-4 text-sm text-ink-soft hover:text-ink disabled:opacity-50">
          {t(windowKeys.log.earlier, "Earlier ↓")}
        </button>
      )}
    </div>
  );
}

/** Reads older pages back until `target`'s line is read, then scrolls it into view, once. */
function Reach({ box, target, more }: { box: RefObject<HTMLDivElement | null>; target: string | null; more: (() => Promise<void>) | null }) {
  const done = useRef(false);
  useEffect(() => {
    if (done.current) return;
    if (target === null) return void more?.();
    done.current = true;
    const line = box.current!.querySelector(`[data-line="${CSS.escape(target)}"]`);
    if (line === null) throw new Error(`log line ${target} is shown, yet not drawn`);
    line.scrollIntoView({ block: "center" });
  });
  return null;
}

function Mark({ icon, tone }: { icon: ReactNode; tone: "info" | "warn" | "error" | "positive" | "muted" }) {
  const tones = { info: "bg-accent-info/15 text-accent-info", warn: "bg-accent-warn/15 text-accent-warn", error: "bg-accent-error/15 text-accent-error", positive: "bg-positive/15 text-positive", muted: "bg-hover text-ink-soft" };
  return <span className={cn("flex size-6 shrink-0 items-center justify-center rounded-full [&_svg]:size-3.5", tones[tone])}>{icon}</span>;
}

function Row({ mark, title, detail, children }: { mark: ReactNode; title: ReactNode; detail?: ReactNode; children?: ReactNode }) {
  return (
    <>
      {mark}
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <p className="text-sm text-ink">{title}</p>
        {detail && <p className="text-xs text-ink-soft">{detail}</p>}
        {children}
      </div>
    </>
  );
}

const FIELD = {
  name: (t) => t(windowKeys.log.field.name, "name"),
  place_id: (t) => t(windowKeys.log.field.placeId, "place id"),
  address: (t) => t(windowKeys.log.field.address, "address"),
  phone: (t) => t(windowKeys.log.field.phone, "phone"),
  website: (t) => t(windowKeys.log.field.website, "website"),
  hours: (t) => t(windowKeys.log.field.hours, "hours"),
  google_primary: (t) => t(windowKeys.log.field.googlePrimary, "Google category"),
  country: (t) => t(windowKeys.log.field.country, "country"),
  point: (t) => t(windowKeys.log.field.point, "point"),
  photos: (t) => t(windowKeys.log.field.photos, "photos"),
  areas: (t) => t(windowKeys.log.field.areas, "service areas"),
  hidden_address: (t) => t(windowKeys.log.field.hiddenAddress, "hidden address"),
  customers_visit: (t) => t(windowKeys.log.field.customersVisit, "customers visit"),
  spots: (t) => t(windowKeys.log.field.spots, "spots"),
  role: (t) => t(windowKeys.log.field.role, "role"),
  owner_gmail: (t) => t(windowKeys.log.field.ownerGmail, "owner gmail"),
} satisfies Record<Field, (t: T) => string>;
const FIELD_NAME = (t: T, field: Field): string => FIELD[field](t);

function shownValue(v: unknown): string {
  if (v === null) return "—";
  if (typeof v === "string") return v;
  return JSON.stringify(v);
}

function Ours({ e, gmail }: { e: LoggedDto; gmail: string | null }) {
  const t = useT();
  const by = e.by === null ? t(windowKeys.log.byGoogle, "by Google, or a run") : t(windowKeys.log.by, "by {name}", { name: e.by });
  const gmailNote = e.of_gmail ? t(windowKeys.log.ofGmail, "the gmail's, {gmail}", { gmail: gmail ?? "" }) : null;
  switch (e.kind) {
    case "added":
      return <Row mark={<Mark tone="info" icon={<Pencil />} />} title={t(windowKeys.log.added, "Added as {state}", { state: STATE_NAME[e.state](t) })} detail={by} />;
    case "state":
      return <Row mark={<Mark tone={e.to === "suspended" ? "error" : e.to === "verified" ? "positive" : "info"} icon={e.to === "suspended" ? <ShieldAlert /> : <ShieldCheck />} />} title={`${STATE_NAME[e.from](t)} → ${STATE_NAME[e.to](t)}`} detail={by} />;
    case "edited":
    case "synced":
      return (
        <Row
          mark={<Mark tone="muted" icon={e.kind === "synced" ? <RefreshCw /> : <Pencil />} />}
          title={e.kind === "synced" ? (e.changes.length === 0 ? t(windowKeys.log.syncedSame, "Sync: nothing changed on Google") : t(windowKeys.log.synced, "Sync: {n} fields changed on Google", { n: e.changes.length })) : t(windowKeys.log.edited, "Edited: {fields}", { fields: e.changes.map((c) => FIELD_NAME(t, c.field)).join(", ") })}
          detail={by}
        >
          {e.changes.length > 0 && (
            <div className="mt-1 flex flex-col gap-0.5 rounded-md bg-hover/60 px-2 py-1.5 font-mono text-xs">
              {e.changes.map((c) => (
                <p key={c.field} className="break-words">
                  <span className="text-ink-soft">{FIELD_NAME(t, c.field)}</span> <span className="text-accent-error">{shownValue(c.from)}</span> → <span className="text-positive">{shownValue(c.to)}</span>
                </p>
              ))}
            </div>
          )}
        </Row>
      );
    case "gmail":
      return <Row mark={<Mark tone="info" icon={<Mail />} />} title={t(windowKeys.log.gmail, "Moved to another gmail")} detail={by} />;
    case "trade":
      return <Row mark={<Mark tone="info" icon={<Tag />} />} title={e.to === null ? t(windowKeys.log.tradeGone, "Its trade taken away") : t(windowKeys.log.trade, "Given another trade")} detail={by} />;
    case "brand":
      return <Row mark={<Mark tone="info" icon={<ImageIcon />} />} title={e.to === null ? t(windowKeys.log.brandGone, "Its brand dropped") : t(windowKeys.log.brand, "Given a brand")} detail={by} />;
    case "film":
      return <Row mark={<Mark tone="muted" icon={<Video />} />} title={e.film === null ? t(windowKeys.log.filmCleared, "Film cleared") : t(windowKeys.log.film, "Film recorded by hand")} detail={[e.film, by].filter(Boolean).join(" · ")} />;
    case "switched":
      return <Row mark={<Mark tone="muted" icon={<Pencil />} />} title={e.on ? t(windowKeys.log.on, "Switched on") : t(windowKeys.log.off, "Switched off")} detail={by} />;
    case "removed":
      return <Row mark={<Mark tone="error" icon={<Pencil />} />} title={t(windowKeys.log.removed, "Removed to the trash")} detail={by} />;
    case "restored":
      return <Row mark={<Mark tone="positive" icon={<Pencil />} />} title={t(windowKeys.log.restored, "Restored from the trash")} detail={by} />;
    case "launched":
      return <Row mark={<Mark tone="info" icon={<MonitorSmartphone />} />} title={e.film ? t(windowKeys.log.launchedFilm, "Browser launched, playing its film") : t(windowKeys.log.launched, "Browser launched")} detail={[gmailNote, by].filter(Boolean).join(" · ")} />;
    case "stopped":
      return <Row mark={<Mark tone="muted" icon={<MonitorSmartphone />} />} title={t(windowKeys.log.stopped, "Browser stopped")} detail={[gmailNote, by].filter(Boolean).join(" · ")} />;
    case "email":
      return <Email e={e} gmail={gmail} />;
  }
}

function Email({ e, gmail }: { e: Extract<LoggedDto, { kind: "email" }>; gmail: string | null }) {
  const t = useT();
  const locale = useLocale();
  const thread = useOpenThread();
  const who = e.counterparts.join(", ");
  const open = () => {
    if (e.gmail_id === null) throw new Error("an email is logged under its gmail");
    void thread.open(e.gmail_id, e.thread);
  };
  return (
    <Row
      mark={<Mark tone="info" icon={<Mail />} />}
      title={e.subject ?? t(windowKeys.log.noSubject, "(no subject)")}
      detail={[e.sent ? t(windowKeys.log.emailTo, "to {who}", { who }) : t(windowKeys.log.emailFrom, "from {who}", { who }), formatDateTime(e.date, locale), e.of_gmail ? t(windowKeys.log.ofGmail, "the gmail's, {gmail}", { gmail: gmail ?? "" }) : null].filter(Boolean).join(" · ")}
    >
      {e.snippet && <p className="line-clamp-2 text-xs text-ink-soft">{e.snippet}</p>}
      <button type="button" disabled={thread.opening !== null} onClick={open} className="flex items-center gap-1 self-start text-xs text-ink-soft hover:text-ink disabled:opacity-50">
        <ExternalLink className="size-3" />
        {t(windowKeys.log.openThread, "Open thread")}
      </button>
    </Row>
  );
}

function Archive({ e, gmb }: { e: GmbEvent; gmb: number }) {
  const t = useT();
  const reviews = (
    <ViewLink to={{ view: "review_archive.mine.gmb", params: { gmb } }} className="shrink-0 text-xs text-ink-soft hover:text-ink">
      {t(windowKeys.profile.reviews, "Reviews")}
    </ViewLink>
  );
  if (e.kind === "scan_failed") return <Row mark={<Mark tone="warn" icon={<ShieldAlert />} />} title={t(windowKeys.log.scanFailed, "A scan of its reviews failed")} detail={e.error} />;
  const who = [e.rating !== null && `★${e.rating}`, e.author].filter(Boolean).join(" ");
  const title = {
    appeared: t(windowKeys.log.appeared, "A review on Google"),
    suspended: t(windowKeys.log.suspended, "A 5-star review suspended"),
    removed: t(windowKeys.log.reviewRemoved, "A review removed"),
    returned: t(windowKeys.log.returned, "A review back on Google"),
    reinstatement_set: t(windowKeys.log.reinstatementSet, "Reinstatement asked of Google"),
    reinstatement_cleared: t(windowKeys.log.reinstatementCleared, "Reinstatement withdrawn"),
  }[e.kind];
  const tone = e.kind === "suspended" || e.kind === "removed" ? "error" : e.kind === "appeared" || e.kind === "returned" ? "info" : "warn";
  return (
    <>
      <Row mark={<Mark tone={tone} icon={<Star />} />} title={title} detail={who} />
      {reviews}
    </>
  );
}

