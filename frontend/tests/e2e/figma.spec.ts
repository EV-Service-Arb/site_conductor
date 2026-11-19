import { test } from "@playwright/test";
import { defineFigmaSuite } from "@evinvest/figma-render";

// Where the crawl starts: the pages no link reaches. `nextjs-portal` is `next dev`'s indicator.
const ASSISTANT = '[aria-label="Assistant"]';
defineFigmaSuite(
  { test },
  [
    { name: "home", url: "/", selector: "main" },
    { name: "assistant", url: "/?assistant=sidebar", selector: ASSISTANT },
    { name: "assistant floating", url: "/?assistant=floating", selector: ASSISTANT },
    { name: "access", url: "/access?continue=%2F&need=sa%3Aoperator", selector: "main" },
  ],
  {
    hide: ["nextjs-portal"],
    // `/account`, `/wallet`: the platform's, which refuses a localhost origin; the rest are no pages
    exclude: [/^\/(account|wallet)\b/, /^\/(auth|api|mcp|grafana)\//],
  },
);
