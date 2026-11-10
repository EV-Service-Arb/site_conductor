"use client";

import {
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  NavDot,
  NavigationMenu,
  NavigationMenuContent,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
  ShellNav,
  TopBar,
  cn,
  navigationMenuTriggerStyle,
  useBinding,
  useSequences,
} from "@evinvest/uikit";
import { ArrowUpRight, Bell, Check, ChevronDown, Globe, MapPin, Menu, PanelLeftClose, PanelLeftOpen, Wallet } from "lucide-react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { type CSSProperties, type PointerEvent, useEffect, useRef, useState } from "react";

import { unreached, useAlerts } from "@/entities/alerts";
import { DevSignInBadge, ownScreens, useMe } from "@/entities/session";
import { useTokens } from "@/entities/tokens";
import { AccountChip } from "@/features/account-chip";
import { AssistantToggle } from "@/features/assistant";
import { useSignOut } from "@/features/sign-out";

import { PANEL } from "@config";

import { safeLocalPath } from "@/shared/api";
import { ROUTES } from "@/shared/config/routes";
import { LOCALES, type Locale, chooseLocale, useLocale, useT } from "@/shared/i18n";

import { toggleSidebar, useSidebarFolded } from "../model/sidebar";
import { type App, useApps } from "./apps";
import { Outline } from "./outline";

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
  const folded = useSidebarFolded();
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
              <Button type="button" variant="ghost" size="sm" icon className="md:hidden" aria-label={t("nav.menu")}>
                <Menu aria-hidden />
              </Button>
            </SheetTrigger>
            <SheetContent side="left" className="w-72 gap-2 p-4">
              <SheetHeader className="flex-row items-center gap-2 p-0">
                <SheetTitle>{t("app.name")}</SheetTitle>
                <DevSignInBadge />
              </SheetHeader>
              {apps.map((a) => (
                <Link key={a.href} href={a.href} onClick={() => setOpen(false)} aria-current={a.here ? "page" : undefined} className={cn(navigationMenuTriggerStyle(), "w-full justify-start px-2", a.here && "bg-hover/50")}>
                  {a.label}
                </Link>
              ))}
              <div className="border-t border-border pt-2">
                <Outline folded={false} onNavigate={() => setOpen(false)} className="h-auto w-auto border-r-0 bg-transparent p-0" />
              </div>
              <OwnScreens className="flex-col items-stretch border-t border-border pt-2" onNavigate={() => setOpen(false)} />
            </SheetContent>
          </Sheet>
          <SidebarToggle />
          <AppSwitcher />
          <DevSignInBadge className="hidden sm:inline-flex" />
          <NavigationMenu viewport={false} className="ml-4 hidden md:flex" aria-label={t("nav.primary")}>
            <NavigationMenuList>
              {apps.map((a) => (
                <AppItem key={a.href} app={a} menu={!(a.here && !folded)} />
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

/** Long enough to cross the gap between the item and its menu. */
const CLOSE_DELAY_MS = 150;

/** An app's link, its outline section hung below it while the mouse is over either; none while the outline shows that section whole. */
function AppItem({ app, menu }: { app: App; menu: boolean }) {
  const pathname = usePathname();
  const [open, setOpen] = useState(false);
  const closing = useRef<number>(undefined);
  useEffect(() => () => window.clearTimeout(closing.current), []);
  const hover = (next: boolean) => (e: PointerEvent) => {
    if (!menu || e.pointerType === "touch") return; // a tap is the link's
    window.clearTimeout(closing.current);
    if (next) setOpen(true);
    else closing.current = window.setTimeout(() => setOpen(false), CLOSE_DELAY_MS);
  };
  return (
    <NavigationMenuItem open={open} onOpenChange={setOpen} onPointerEnter={hover(true)} onPointerLeave={hover(false)}>
      <NavigationMenuLink asChild className={cn(navigationMenuTriggerStyle(), "group flex-row items-center gap-1", app.here && "bg-hover/50")}>
        <Link href={app.href} aria-current={app.here ? "page" : undefined} {...(menu && { "aria-expanded": open, "data-state": open ? "open" : "closed" })}>
          {app.label}
          {menu && <ChevronDown aria-hidden className="size-3.5 text-ink-soft transition-transform group-data-[state=open]:rotate-180" />}
        </Link>
      </NavigationMenuLink>
      {/* any click in it has routed, as in the outline */}
      {menu && <NavigationMenuContent className="p-0" onClick={() => setOpen(false)}>
        <ShellNav
          groups={[app.section]}
          pathname={pathname}
          linkComponent={Link}
          labels={{ primary: app.label }}
          style={{ "--shell-rail-w": "18rem" } as CSSProperties}
          className="max-h-[70svh] gap-0 border-r-0 bg-transparent p-2"
        />
      </NavigationMenuContent>}
    </NavigationMenuItem>
  );
}

/** The outline beside the page, whole or folded to its icons; a phone has it in the Sheet instead. */
function SidebarToggle() {
  const t = useT();
  const folded = useSidebarFolded();
  const keys = useSequences().get("sidebar");
  useBinding({ on: { key: "s" }, run: { id: "sidebar", label: t("keys.sidebar"), run: toggleSidebar } });
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
      aria-label={t("keys.sidebar")}
      title={keys === undefined ? t("keys.sidebar") : `${t("keys.sidebar")} · ${keys}`}
    >
      <Icon aria-hidden />
    </Button>
  );
}

/** Service-Arb, then the configured links. */
function AppSwitcher() {
  const t = useT();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className={cn(navigationMenuTriggerStyle(), "shrink-0 gap-1 px-2")} aria-label={t("apps.switch")}>
        <span className="text-base font-semibold whitespace-nowrap text-ink">{t("app.name")}</span>
        <ChevronDown aria-hidden className="size-4 text-ink-soft transition-transform group-data-[state=open]:rotate-180" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" sideOffset={4} className="w-56">
        <DropdownMenuItem asChild>
          <Link href="/" aria-current="true">
            <MapPin aria-hidden className="size-4" />
            {t("app.name")}
            <Check aria-hidden className="ml-auto size-4 text-ink-soft" />
          </Link>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        {PANEL.links.map((l) => (
          <DropdownMenuItem key={l.name} asChild>
            <a href={l.href} {...(l.newTab && { target: "_blank", rel: "noreferrer" })}>
              {/* a mask, so a brand's mark paints in the menu's ink like the lucide icons beside it */}
              <span aria-hidden className="size-4 shrink-0 bg-current mask-contain mask-center mask-no-repeat" style={{ maskImage: `url(${l.icon})` }} />
              {l.name}
              {l.newTab && <ArrowUpRight aria-hidden className="ml-auto size-4 text-ink-soft" />}
            </a>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** The bell: the Alerts page, with a dot while nothing reaches the caller. */
function AlertsLink() {
  const t = useT();
  const alerts = useAlerts();
  return (
    <Button asChild variant="ghost" size="sm" icon className="relative">
      <Link href={ROUTES.alerts} aria-label={t("nav.alerts")}>
        <Bell aria-hidden />
        {alerts.status === "ok" && unreached(alerts.data) && <NavDot corner tone="warn" label={t("alerts.connect")} />}
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
        <Button type="button" variant="ghost" size="sm" icon aria-label={t("nav.language")}>
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
  const label = { sources: t("nav.sources"), usage: t("usage.title") };
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant="ghost" size="sm" icon className="hidden md:inline-flex" aria-label={t("nav.menu")}>
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
    sources: t("nav.sources"),
    usage: t("usage.title"),
    tokens: tokens.status === "ok" ? t("tokens.count", { n: tokens.data.balance }) : t("nav.tokens"),
    wallet: t("nav.wallet"),
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
