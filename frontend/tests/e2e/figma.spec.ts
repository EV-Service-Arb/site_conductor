import { test } from "@playwright/test";
import { defineFigmaSuite } from "@evinvest/figma-render";

// The conductor's own screens. Not `/sites`, `/review_archive`: other services' bundles the stub does
// not serve; nor `/account`, `/wallet`: the platform's, which refuses a localhost origin.
// `nextjs-portal` is `next dev`'s indicator.
const ASSISTANT = '[aria-label="Assistant"]';
defineFigmaSuite(
  { test },
  [
    { name: "home", url: "/", selector: "main" },
    { name: "assistant", url: "/?assistant=sidebar", selector: ASSISTANT },
    { name: "assistant floating", url: "/?assistant=floating", selector: ASSISTANT },
    { name: "assistant full", url: "/assistant/", selector: "main" },
    { name: "sources", url: "/sources/", selector: "main" },
    { name: "usage", url: "/usage/", selector: "main" },
    { name: "tokens", url: "/tokens/", selector: "main" },
    { name: "alerts", url: "/alerts/", selector: "main" },
    { name: "admin tokens", url: "/admin/tokens/", selector: "main" },
    { name: "access", url: "/access/?need=sa:playbook:mcp:use", selector: "main" },
  ],
  { hide: ["nextjs-portal"] },
);
