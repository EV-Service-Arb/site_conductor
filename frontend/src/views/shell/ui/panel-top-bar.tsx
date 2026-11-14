"use client";

import {
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
  NavDot,
  type NavGroup,
  type NavIcon,
  NavMenu,
  NavMenuList,
  NavigationMenu,
  NavigationMenuList,
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
  TopBar,
  cn,
  navigationMenuTriggerStyle,
  useBinding,
  useKeymap,
} from "@evinvest/uikit";
import { ArrowUpRight, Bell, Globe, MapPin, Menu, PanelLeftClose, PanelLeftOpen, Wallet } from "lucide-react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";

import { unreached, useAlerts, alerts as alertsKeys } from "@/entities/alerts";
import { DevSignInBadge, ownScreens, useMe } from "@/entities/session";
import { useTokens, tokens as tokensKeys } from "@/entities/tokens";
import { AccountChip } from "@/features/account-chip";
import { AssistantToggle } from "@/features/assistant";
import { useSignOut } from "@/features/sign-out";

import { PANEL } from "@config";

import { safeLocalPath } from "@/shared/api";
import { ROUTES } from "@/shared/config/routes";
import { LOCALES, type Locale, chooseLocale, useLocale, useT, nav, keys as keysKeys, usage } from "@/shared/i18n";

import { toggleSidebar, useSidebarFolded } from "../model/sidebar";
import { type App, useApps } from "./apps";
import { Outline } from "./outline";
import { app as appKeys, apps as appsKeys } from "../model/i18n";

/**
 * `[sidebar] [Service-Arb⌄] [Sites] [Review Archive] … [tokens] [wallet] [assistant] [bell] [language] [chip] [≡ Sources, Usage]`; on a phone
 * the screens go into a Sheet, with the outline. The end slot is right-anchored, so the tokens appearing or changing width
 * grow leftwards and never move the icons.
 */
export function PanelTopBar() {
  const t = useT();
  const me = useMe();
  const signOut = useSignOut();
  const router = useRouter();
  const pathname = usePathname();
  const apps = useApps();
  const switcher = useSwitcherGroups();
  const [open, setOpen] = useState(false);

  useEffect(() => {
    // a sites element's link it does not route itself; the account chip's own bubbles from the chip
    const onNavigate = (e: Event) => {
      if (e.target !== window) return;
      const href: unknown = (e as CustomEvent<{ href?: unknown }>).detail?.href;
      const path = typeof href === "string" ? safeLocalPath(href) : null;
      if (path === null) throw new Error(`ev:navigate: not a path on this origin: ${String(href)}`);
      e.preventDefault();
      setOpen(false);
      router.push(path);
    };
    window.addEventListener("ev:navigate", onNavigate);
    return () => window.removeEventListener("ev:navigate", onNavigate);
  }, [router]);

  return (
    <TopBar
      className="px-2"
      start={
        <>
          <Sheet open={open} onOpenChange={setOpen}>
            <SheetTrigger asChild>
              <Button type="button" variant="ghost" size="sm" icon className="md:hidden" aria-label={t(nav.menu, "Menu")}>
                <Menu aria-hidden />
              </Button>
            </SheetTrigger>
            <SheetContent side="left" className="w-72 gap-2 p-4">
              <SheetHeader className="flex-row items-center gap-2 p-0">
                <SheetTitle>{t(appKeys.name, "Service-Arb")}</SheetTitle>
                <DevSignInBadge />
              </SheetHeader>
              <NavMenuList groups={switcher} pathname={pathname} linkComponent={Link} onNavigate={() => setOpen(false)} className="w-auto" />
              <div className="border-t border-border pt-2">
                <Outline folded={false} onNavigate={() => setOpen(false)} className="h-auto w-auto border-r-0 bg-transparent p-0" />
              </div>
              <OwnScreens className="flex-col items-stretch border-t border-border pt-2" onNavigate={() => setOpen(false)} />
            </SheetContent>
          </Sheet>
          <SidebarToggle />
          <AppSwitcher />
          <DevSignInBadge className="hidden sm:inline-flex" />
          <NavigationMenu viewport={false} className="ml-4 hidden md:flex" aria-label={t(nav.primary, "Main navigation")}>
            <NavigationMenuList>
              {apps.map((a) => (
                <AppItem key={a.href} app={a} menu={!a.here} />
              ))}
            </NavigationMenuList>
          </NavigationMenu>
        </>
      }
      end={
        <div className="flex items-center gap-1">
          <OwnScreens bar className="hidden md:flex" />
          {PANEL.assistant && !`${pathname}/`.startsWith(ROUTES.assistant) && <AssistantToggle />}
          <AlertsLink />
          <LanguageMenu />
          <AccountChip me={me} onSignOut={signOut} className="ml-1" />
          <ScreensMenu />
        </div>
      }
    />
  );
}

/** The open app has none: the outline shows its views, whole or in its icons' flyouts, so the menu would only repeat them. */
function AppItem({ app, menu }: { app: App; menu: boolean }) {
  const pathname = usePathname();
  return (
    <NavMenu label={app.label} href={app.href} current={app.here} linkComponent={Link}>
      {menu ? "content" in app.section ? app.section.content : <NavMenuList groups={[app.section]} pathname={pathname} linkComponent={Link} /> : undefined}
    </NavMenu>
  );
}

/** The outline beside the page, whole or folded to its icons; a phone has it in the Sheet instead. */
function SidebarToggle() {
  const t = useT();
  const folded = useSidebarFolded();
  const keys = useKeymap().bound.get("sidebar")?.sequence;
  useBinding({ on: { key: "s" }, id: "sidebar", run: { label: t(keysKeys.sidebar, "Sidebar: fold or unfold"), run: toggleSidebar } });
  const Icon = folded ? PanelLeftOpen : PanelLeftClose;
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      icon
      className="hidden md:inline-flex"
      onClick={toggleSidebar}
      aria-pressed={folded}
      aria-label={t(keysKeys.sidebar, "Sidebar: fold or unfold")}
      title={keys === undefined ? t(keysKeys.sidebar, "Sidebar: fold or unfold") : `${t(keysKeys.sidebar, "Sidebar: fold or unfold")} · ${keys}`}
    >
      <Icon aria-hidden />
    </Button>
  );
}

/** Each link with its mark, painted in the row's ink like the lucide icons beside it. */
const LINKS = PANEL.links.map((l) => ({
  ...l,
  icon: function LinkIcon({ className }: { className?: string }) {
    return <span aria-hidden className={cn("bg-current mask-contain mask-center mask-no-repeat", className)} style={{ maskImage: `url(${l.icon})` }} />;
  } satisfies NavIcon,
}));

/** Service-Arb's home and apps, then the configured links: the switcher's menu, and the phone's Sheet. */
function useSwitcherGroups(): NavGroup[] {
  const t = useT();
  const apps = useApps();
  return [
    {
      id: "apps",
      label: t(appKeys.name, "Service-Arb"),
      items: [
        { id: "home", href: "/", label: t(nav.home, "Home"), icon: MapPin, description: t(nav.home.description, "Every GBP on one map"), match: "exact" },
        ...apps.map((a) => ({ id: a.href, href: a.href, label: a.label, icon: a.icon, description: a.description })),
      ],
    },
    {
      id: "elsewhere",
      label: t(appsKeys.elsewhere, "Elsewhere"),
      items: LINKS.map((l) => ({
        id: l.name,
        href: l.href,
        label: l.name,
        icon: l.icon,
        description: l.description(t),
        external: true,
        ...(l.newTab && { target: "_blank" as const, trailing: <ArrowUpRight aria-hidden className="size-4 shrink-0 text-ink-soft" /> }),
      })),
    },
  ];
}

function AppSwitcher() {
  const t = useT();
  const pathname = usePathname();
  const groups = useSwitcherGroups();
  return (
    <NavigationMenu viewport={false} aria-label={t(appsKeys.switch, "Switch service")}>
      <NavigationMenuList>
        <NavMenu label={<span className="text-base font-semibold whitespace-nowrap text-ink">{t(appKeys.name, "Service-Arb")}</span>} href="/" current={pathname === "/"} linkComponent={Link} className="shrink-0 px-2">
          <NavMenuList groups={groups} pathname={pathname} linkComponent={Link} />
        </NavMenu>
      </NavigationMenuList>
    </NavigationMenu>
  );
}

/** The bell: the Alerts page, with a dot while nothing reaches the caller. */
function AlertsLink() {
  const t = useT();
  const alerts = useAlerts();
  return (
    <Button asChild variant="ghost" size="sm" icon className="relative">
      <Link href={ROUTES.alerts} aria-label={t(nav.alerts, "Alerts")}>
        <Bell aria-hidden />
        {alerts.status === "ok" && unreached(alerts.data) && <NavDot corner tone="warn" label={t(alertsKeys.connect, "Connect Telegram")} />}
      </Link>
    </Button>
  );
}

function LanguageMenu() {
  const t = useT();
  const locale = useLocale();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant="ghost" size="sm" icon aria-label={t(nav.language, "Language")}>
          <Globe aria-hidden />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuRadioGroup value={locale} onValueChange={(l) => chooseLocale(l as Locale)}>
          {Object.entries(LOCALES).map(([l, name]) => (
            <DropdownMenuRadioItem key={l} value={l} lang={l}>
              {name}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** The screens the bar keeps behind its menu; a phone's Sheet lists them with the rest. */
const inMenu = (id: string): id is "sources" | "usage" => id === "sources" || id === "usage";

function ScreensMenu() {
  const t = useT();
  const me = useMe();
  const pathname = usePathname();
  const label = { sources: t(nav.sources, "Sources"), usage: t(usage.title, "Usage") };
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant="ghost" size="sm" icon className="hidden md:inline-flex" aria-label={t(nav.menu, "Menu")}>
          <Menu aria-hidden />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        {ownScreens(me, PANEL.walletPage)
          .filter(inMenu)
          .map((id) => (
            <DropdownMenuItem key={id} asChild>
              <Link href={ROUTES[id]} aria-current={pathname.startsWith(ROUTES[id]) ? "page" : undefined}>
                {label[id]}
              </Link>
            </DropdownMenuItem>
          ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** The caller's own screens; Tokens is labelled with the count, never called a balance: evinvest.ltd's wallet is another ledger. */
function OwnScreens({ bar = false, className, onNavigate }: { bar?: boolean; className?: string; onNavigate?: () => void }) {
  const t = useT();
  const me = useMe();
  const tokens = useTokens();
  const pathname = usePathname();
  const label = {
    sources: t(nav.sources, "Sources"),
    usage: t(usage.title, "Usage"),
    tokens: tokens.status === "ok" ? t(tokensKeys.count, "{n} tokens", { n: tokens.data.balance }) : t(nav.tokens, "Tokens"),
    wallet: t(nav.wallet, "Wallet"),
  };
  return (
    <div className={cn("flex items-center gap-1", className)}>
      {ownScreens(me, PANEL.walletPage)
        .filter((id) => !(bar && inMenu(id)))
        .map((id) => {
        const here = pathname.startsWith(ROUTES[id]);
        return (
          <Link key={id} href={ROUTES[id]} onClick={() => onNavigate?.()} aria-current={here ? "page" : undefined} className={cn(navigationMenuTriggerStyle(), "justify-start gap-2", id === "wallet" && "md:size-8 md:justify-center md:px-0", here && "bg-hover/50")}>
            {id === "wallet" && <Wallet aria-hidden className="size-4" />}
            {/* the wallet is an icon in the bar, a labelled row in the sheet */}
            <span className={cn(id === "tokens" && "tabular-nums", id === "wallet" && "md:sr-only")}>{label[id]}</span>
          </Link>
        );
      })}
    </div>
  );
}
