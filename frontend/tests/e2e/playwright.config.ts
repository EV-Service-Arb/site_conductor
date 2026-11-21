import { basename } from "node:path";

import { defineConfig, devices } from "@playwright/test";

import type { Stack } from "./stack";

export const root = new URL("../../../", import.meta.url).pathname;
export const port = (name: string): number => {
  const value = process.env[name];
  if (!value) throw new Error(`${name}: the flake's localEnv sets it; run through \`nix run .#e2e\` or \`nix run .#figma\``);
  return Number(value.replace(/^.*:/, ""));
};

// The real stack on ports of its own (its conductor, dev sign-in as an admin), never a running one's servers: those may
// face the real backend, as an operator. It outlives the run, reused by the next (stack.ts). Run through the flake, which
// supplies `@playwright/test` with its pinned browsers.
export function stackConfig({ stack, specs }: { stack: Stack; specs: { testMatch: string } | { testIgnore: string } }) {
  const tmp = (path: string) => `${root}frontend/tmp/playwright-${basename(stack.state)}/${path}`;
  process.env.STACK_TIMELINE = `${stack.state}/timeline.tsv`; // where local-stack keeps it; read by timeline.ts, written by sign-in.ts too
  return defineConfig({
    testDir: ".",
    ...specs,
    fullyParallel: true, // the specs share no server state: each page is its own context, over the one session
    workers: process.env.CI ? "100%" : undefined, // a runner's cores are the run's alone; a machine's are the stack's too
    outputDir: tmp(""),
    reporter: [["list"], ["./timeline.ts", { out: tmp("timeline.txt") }]],
    globalSetup: ["./stack.ts", "./sign-in.ts"],
    metadata: { stack },
    use: {
      baseURL: stack.web,
      deviceScaleFactor: 1,
      storageState: tmp("session.json"),
    },
    projects: [
      { name: "desktop", use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } } },
      { name: "mobile", use: { ...devices["Desktop Chrome"], viewport: { width: 390, height: 844 } } },
    ],
  });
}

// the specs touch the conductor's own screens alone; no Google accounts: never spends on, nor reads, a person's
const E2E = port("E2E_STACK_PORT");
export default stackConfig({
  stack: {
    argv: ["--port", String(E2E), "--role", "admin", "--no-peers"],
    env: { SA_CONDUCTOR_CONFIG: new URL("./conductor.nix", import.meta.url).pathname },
    state: `${root}.local-port-${E2E}`,
    web: `http://localhost:${E2E + 1}`, // `next dev` refuses its dev resources to any other host, so the page never hydrates
    health: `http://127.0.0.1:${E2E}/health`,
    fresh: "watched",
  },
  specs: { testIgnore: "figma.spec.ts" },
});
