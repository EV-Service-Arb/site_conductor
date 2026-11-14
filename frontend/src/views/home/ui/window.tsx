"use client";

import { Badge, Button, cn } from "@evinvest/uikit";
import { ChevronDown, ChevronUp, Globe, Hash, Mail, MapPin, Phone, QrCode, Send, Smartphone, X } from "lucide-react";
import type { ReactNode } from "react";

import { CategoryChip, type Listed, MUST_HAVES, TABS, type Tab, open, openAccount, readiness, useGmbTags, useSelection, windowKeys, STATE_NAME } from "@/entities/gmb";
import { type T, useT, account } from "@/shared/i18n";
import { setQuery } from "@/shared/lib/query";
import { useButtonSize } from "@/shared/ui/touch";

import { BrowserTab } from "./browser-tab";
import { KitTab } from "./kit-tab";
import { Launch } from "./launch";

const TAB_NAME = {
  launch: (t) => t(windowKeys.tab.launch, "Launch"),
  browser: (t) => t(windowKeys.tab.browser, "Browser"),
  kit: (t) => t(windowKeys.tab.kit, "Review kit"),
} satisfies Record<Tab, (t: T) => string>;

const ICON: Record<Tab, ReactNode> = { launch: <Send aria-hidden className="size-4" />, browser: <Smartphone aria-hidden className="size-4" />, kit: <QrCode aria-hidden className="size-4" /> };

/** A GBP's window over the map, beside the outline: its header, and the tabs its state has. */
export function LocationWindow({ listed, tab }: { listed: Listed; tab: Tab }) {
  const t = useT();
  const button = useButtonSize();
  const selection = useSelection();
  const { tags } = useGmbTags();
  const { gmb, gmail } = listed;
  const site = tags.sites?.get(gmb.id);
  // the outline's order, of the GBPs a window opens on
  const order = selection === null ? [] : [...selection.selected, ...selection.rows.filter((l) => !selection.isSelected(l.gmb.id))].filter((l) => TABS[l.gmb.state].length > 0);
  const at = order.findIndex((l) => l.gmb.id === gmb.id);
  const step = (by: number) => {
    const next = order[(at + by + order.length) % order.length];
    if (next !== undefined) open(next.gmb);
  };
  const ready = gmb.state === "draft" ? readiness(listed, tags).filter((r) => r.ready).length : null;
  const point = gmb.address ?? (gmb.location && t(windowKeys.planned, "planned · {at}", { at: `${gmb.location.lat.toFixed(3)}, ${gmb.location.lng.toFixed(3)}` }));
  const facts = [
    point && { icon: <MapPin aria-hidden />, text: point },
    gmb.phone && { icon: <Phone aria-hidden />, text: gmb.phone },
    site && { icon: <Globe aria-hidden />, text: `${site.brand}/${site.location}` },
    gmail && { icon: <Mail aria-hidden />, text: gmail.gmail, open: () => openAccount({ gmail: gmail.id, tab: "connections" }) },
    gmb.place_id && { icon: <Hash aria-hidden />, text: gmb.place_id },
  ].filter((f) => !!f);

  return (
    <section aria-label={gmb.name} className="absolute inset-y-0 right-0 left-0 flex flex-col overflow-y-auto border-l border-border bg-background md:left-36">
      <header className="flex flex-col gap-1 px-6 pt-4">
        <div className="flex items-center gap-3">
          <h2 className="min-w-0 truncate text-lg font-semibold text-ink">{gmb.name}</h2>
          <Badge>{STATE_NAME[gmb.state](t)}</Badge>
          <CategoryChip gmb={gmb} />
          {ready !== null && (
            <span className="text-sm text-ink-soft">
              <span className={cn("font-mono tabular-nums", ready === MUST_HAVES.length ? "text-positive" : "text-accent-warn")}>
                {ready}/{MUST_HAVES.length}
              </span>{" "}
              {t(windowKeys.readyToSubmit, "ready to submit")}
            </span>
          )}
          <span className="flex-1" />
          {at >= 0 && order.length > 1 && (
            <span className="flex items-center gap-1 font-mono text-xs text-ink-soft tabular-nums">
              {at + 1} / {order.length}
              <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.prev, "Previous")} title={t(windowKeys.prev, "Previous")} onClick={() => step(-1)}>
                <ChevronUp aria-hidden />
              </Button>
              <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.next, "Next")} title={t(windowKeys.next, "Next")} onClick={() => step(1)}>
                <ChevronDown aria-hidden />
              </Button>
            </span>
          )}
          <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.close, "Close the window")} title={t(windowKeys.close, "Close the window")} onClick={() => setQuery({ tab: null })}>
            <X aria-hidden />
          </Button>
        </div>
        <p className="flex flex-wrap gap-x-4 gap-y-1 text-sm text-ink-soft [&_svg]:size-3.5">
          {facts.map((f) =>
            "open" in f ? (
              <button key={f.text} type="button" onClick={f.open} title={t(account.open, "Connections of {gmail}", { gmail: f.text })} className="flex items-center gap-1.5 rounded outline-none hover:text-ink hover:underline focus-visible:ring-2 focus-visible:ring-ring">
                {f.icon}
                {f.text}
              </button>
            ) : (
              <span key={f.text} className="flex items-center gap-1.5">
                {f.icon}
                {f.text}
              </span>
            ),
          )}
        </p>
      </header>
      <nav role="tablist" aria-label={t(windowKeys.tabs, "Sections")} className="flex gap-1 border-b border-border px-6 pt-3">
        {TABS[gmb.state].map((k) => (
          <button
            key={k}
            type="button"
            role="tab"
            aria-selected={k === tab}
            // a GBP's browser is its gmail's
            disabled={k === "browser" && gmail === null}
            title={k === "browser" && gmail === null ? t(windowKeys.browser.needsGmail, "Needs a gmail") : undefined}
            onClick={() => setQuery({ tab: k })}
            className={cn(
              "-mb-px flex items-center gap-2 border-b-2 px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring",
              k === tab ? "border-ink text-ink" : "border-transparent text-ink-soft hover:text-ink disabled:opacity-50 disabled:hover:text-ink-soft",
            )}
          >
            {ICON[k]}
            {TAB_NAME[k](t)}
          </button>
        ))}
      </nav>
      <div role="tabpanel" className="p-6">
        {tab === "launch" && <Launch listed={listed} />}
        {tab === "browser" && <BrowserTab listed={listed} />}
        {tab === "kit" && <KitTab listed={listed} />}
      </div>
    </section>
  );
}
