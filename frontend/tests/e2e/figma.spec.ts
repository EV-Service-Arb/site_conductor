import { test } from "@playwright/test";
import { defineFigmaSuite, type Section } from "@evinvest/figma-render";
import { href } from "@evinvest/views";

import { VIEWS } from "../../src/shared/api/views";

// Where the crawl starts: every view a URL alone opens, each window Home opens by a flag, and the states no link reaches.
const table = VIEWS as unknown as Parameters<typeof href>[0];
const at = (view: string, params: Record<string, unknown>) => href(table, { view, params } as never);
const views: Section[] = Object.entries(VIEWS.views).flatMap(([view, v]) =>
  v.params.required.length > 0 || v.query.required.length > 0 ? [] : [{ name: view, url: at(view, {}), selector: "main" }],
);
const windows: Section[] = Object.entries(VIEWS.views.home.query.properties).flatMap(([key, p]) =>
  "type" in p && p.type === "boolean" ? [{ name: `home ${key}`, url: at("home", { [key]: true }), selector: "main" }] : [],
);
const ASSISTANT = '[aria-label="Assistant"]';
defineFigmaSuite(
  { test },
  [
    ...views,
    ...windows,
    { name: "assistant", url: "/?assistant=sidebar", selector: ASSISTANT },
  ],
  {
    hide: ["nextjs-portal"], // `next dev`'s indicator
    // `/user/account`, `/user/wallet`: the platform's, which refuses a localhost origin; `assistant=`: carried into every link from
    // a page that shows it; the rest are no pages
    exclude: [/^\/user\/(account|wallet)\b/, /[?&]assistant=/, /^\/(auth|api|mcp|grafana)\//],
    oneDocument: /^\/(\?|$)/, // Home: a load of it buys a Google map
  },
);
