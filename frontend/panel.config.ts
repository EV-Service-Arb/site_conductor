import type { MessageKey } from "@/shared/i18n";

// The front end's switches, compiled into the bundle: the image and `nix run .#dev` build the same one.

type PanelConfig = {
  /** The sign-in dialog over the panel, the platform's account chip and sign-in; concierge's bundle from /api/auth/mfe/*. Off: a 401 redirects to sign in. */
  signinOverlay: boolean;
  /** The wallet page, its bundle from /wallet/mfe/*. Off: no menu entry, the route says the wallet is unavailable. */
  walletPage: boolean;
  /** What review_archive opens as; `?grafana=true|false` switches a browser tab. */
  reviewArchiveUi: "grafana" | "legacy";
  /** `oldschool`: the kit's pre-0.31 navy look, kept to roll back to. */
  theme: "dark" | "oldschool";
  /** The assistant beside every signed-in page, toggled from the top bar; a preview whose answers are canned. */
  assistant: boolean;
  /** The app switcher's entries under Service-Arb, in order; `icon` a monochrome mark under `public/`. */
  links: { name: string; href: string; icon: string; description: MessageKey; newTab: boolean }[];
};

export const PANEL: PanelConfig = {
  signinOverlay: true,
  walletPage: true,
  reviewArchiveUi: "grafana",
  theme: "dark",
  assistant: true,
  links: [
    { name: "Discord", href: "https://discord.gg/nGU6Fk4SAK", icon: "/links/discord.svg", description: "links.discord", newTab: true },
    { name: "GitHub", href: "https://github.com/EV-Service-Arb", icon: "/links/github.svg", description: "links.github", newTab: true },
    { name: "EV Investment", href: "https://evinvest.ltd", icon: "/links/ev.svg", description: "links.evInvestment", newTab: false },
    { name: "EV Estate", href: "https://estate.evinvest.ltd", icon: "/links/ev.svg", description: "links.evEstate", newTab: false },
  ],
};
