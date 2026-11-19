"use client";

import { useResource } from "@evinvest/api/react";
import { Skeleton } from "@evinvest/uikit";
import { type LucideIcon, Mail, MapPin, Plus, QrCode, Store, Tag, Trash2 } from "lucide-react";
import { type ComponentType, useEffect } from "react";

import { type KeyOf, type Kind, type WinKey, closeWindow, gmbs as gmbsKeys, kit as kitKeys, narrowed, scopeKeys, trash as trashKeys, under, useBrands, useGmails, useTrades, useTrash } from "@/entities/gmb";
import { AddPanel } from "@/features/add-gmb";
import { api } from "@/shared/api";
import { type T, useT } from "@/shared/i18n";
import { AccountWindow } from "@/views/home/ui/account-window";
import { BrandWindow } from "@/views/home/ui/brand-window";
import { GbpBody, GbpTitle } from "@/views/home/ui/gbp-window";
import { CategoriesWindow, TradeWindow } from "@/views/home/ui/trade-window";
import { TrashWindow } from "@/views/home/ui/trash-window";
import { KitBody, KitTitle } from "@/views/kit";

interface Face<K extends Kind> {
  icon: LucideIcon;
  /** Its width over the map, in px. */
  width: number | ((k: KeyOf<K>) => number);
  /** Over the map its title bar's, as a page its header's. */
  Title: ComponentType<{ k: KeyOf<K>; page: boolean }>;
  /** The same over the map and as a page. */
  Body: ComponentType<{ k: KeyOf<K>; page: boolean }>;
}

const Loading = () => <Skeleton className="m-4 h-64" />;

function AccountBody({ k }: { k: KeyOf<"account"> }) {
  const gmails = useGmails();
  const gmail = gmails.status === "ok" ? (gmails.data.gmails.find((g) => g.id === k.gmail) ?? null) : undefined;
  // removed, here or in another tab
  useEffect(() => {
    if (gmail === null) closeWindow(k);
  }, [gmail, k]);
  if (gmails.status !== "ok" || gmail === undefined) return <Loading />;
  if (gmail === null) return null;
  return <AccountWindow gmail={gmail} tab={k.tab} gbps={under(gmails.data, gmail.id).length} />;
}

function TradeBody({ k }: { k: KeyOf<"trade"> }) {
  const gmails = useGmails();
  const trades = useTrades();
  const trade = trades.status === "ok" ? (trades.data.find((tr) => tr.id === k.trade) ?? null) : undefined;
  // deleted, here or in another tab
  useEffect(() => {
    if (trade === null) closeWindow(k);
  }, [trade, k]);
  if (gmails.status !== "ok" || trade === undefined) return <Loading />;
  if (trade === null) return null;
  return <TradeWindow trade={trade} country={k.country} gmbs={gmails.data.gmbs} gmails={gmails.data.gmails} />;
}

function BrandBody({ k }: { k: KeyOf<"brand"> }) {
  const gmails = useGmails();
  const trades = useTrades();
  const brands = useBrands();
  const brand = brands.status === "ok" ? (brands.data.find((b) => b.id === k.brand) ?? null) : undefined;
  // removed, here or in another tab
  useEffect(() => {
    if (brand === null) closeWindow(k);
  }, [brand, k]);
  if (gmails.status !== "ok" || trades.status !== "ok" || brand === undefined) return <Loading />;
  if (brand === null) return null;
  const trade = trades.data.find((tr) => tr.id === brand.trade_id);
  if (trade === undefined) throw new Error(`brand ${brand.id}'s trade ${brand.trade_id} is not read: a trade holding a live brand is not deleted`);
  return <BrandWindow trade={trade} brand={brand} gmbs={gmails.data.gmbs} gmails={gmails.data.gmails} />;
}

function CategoriesBody() {
  const gmails = useGmails();
  const trades = useTrades();
  if (gmails.status !== "ok" || trades.status !== "ok") return <Loading />;
  return <CategoriesWindow trades={trades.data} gmbs={gmails.data.gmbs} />;
}

function TrashBody() {
  const trash = useTrash();
  if (trash.status !== "ok") return <Loading />;
  return <TrashWindow trash={trash.data} />;
}

function Named({ text }: { text: string }) {
  return <span className="truncate">{text}</span>;
}

export const FACES = {
  // the operational tabs (02) lay out side by side; Profile and Log read as a column, as wide as the tab strip
  gbp: { icon: MapPin, width: (k) => (k.tab === "profile" || k.tab === "log" ? 760 : 1000), Title: GbpTitle, Body: GbpBody },
  account: {
    icon: Mail,
    width: 768,
    Title: function AccountTitle({ k }) {
      return <Named text={useWindowLabel()(k)} />;
    },
    Body: AccountBody,
  },
  trade: {
    icon: Tag,
    width: 768,
    Title: function TradeTitle({ k }) {
      return <Named text={useWindowLabel()(k)} />;
    },
    Body: TradeBody,
  },
  brand: {
    icon: Store,
    width: 768,
    Title: function BrandTitle({ k }) {
      return <Named text={useWindowLabel()(k)} />;
    },
    Body: BrandBody,
  },
  categories: {
    icon: Tag,
    width: 768,
    Title: function CategoriesTitle({ k }) {
      return <Named text={useWindowLabel()(k)} />;
    },
    Body: CategoriesBody,
  },
  trash: {
    icon: Trash2,
    width: 768,
    Title: function TrashTitle({ k }) {
      return <Named text={useWindowLabel()(k)} />;
    },
    Body: TrashBody,
  },
  add: {
    icon: Plus,
    width: 448,
    Title: function AddTitle({ k }) {
      return <Named text={useWindowLabel()(k)} />;
    },
    Body: function AddBody({ k, page }) {
      return <AddPanel key={JSON.stringify(k.at)} at={k.at} page={page} />;
    },
  },
  kit: {
    icon: QrCode,
    width: 896,
    Title: ({ k }) => <KitTitle at={k.at} />,
    Body: ({ k, page }) => <KitBody at={k.at} page={page} />,
  },
} satisfies { [K in Kind]: Face<K> };

export const faceOf = (k: WinKey) => FACES[k.kind] as Face<Kind>;

const LABEL = {
  categories: (t) => t(scopeKeys.categories, "Categories"),
  trash: (t) => t(trashKeys.title, "Trash"),
  add: (t) => t(gmbsKeys.gmb.add, "Add a GBP"),
  kits: (t) => t(kitKeys.list, "Review kits"),
} satisfies Record<string, (t: T) => string>;

/** A window's name in the hotbar and its title: what it shows, by name once read. */
export function useWindowLabel(): (k: WinKey) => string {
  const t = useT();
  const gmails = useGmails();
  const trades = useTrades();
  const brands = useBrands();
  const kits = useResource(api, "/kits");
  return (k) => {
    switch (k.kind) {
      case "gbp": {
        const gmb = gmails.status === "ok" ? narrowed(gmails.data, null, "").find((l) => l.gmb.id === k.gmb) : undefined;
        return gmb?.gmb.name ?? `GBP ${k.gmb}`;
      }
      case "account":
        return (gmails.status === "ok" ? gmails.data.gmails.find((g) => g.id === k.gmail)?.gmail : undefined) ?? `gmail ${k.gmail}`;
      case "trade":
        return [(trades.status === "ok" ? trades.data.find((tr) => tr.id === k.trade)?.name : undefined) ?? `trade ${k.trade}`, k.country].filter(Boolean).join(" · ");
      case "brand":
        return (brands.status === "ok" ? brands.data.find((b) => b.id === k.brand)?.name : undefined) ?? `brand ${k.brand}`;
      case "categories":
      case "trash":
      case "add":
        return LABEL[k.kind](t);
      case "kit":
        return k.at.kit === "" ? LABEL.kits(t) : ((kits.status === "ok" ? kits.data.kits.find((x) => x.slug === k.at.kit)?.name : undefined) ?? k.at.kit);
    }
  };
}
