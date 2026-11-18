import { defineConfig, devices } from "@playwright/test";

const port = (name: string): number => {
  const value = process.env[name];
  if (!value) throw new Error(`${name}: the flake's localEnv sets it; run through \`nix run .#e2e\` or \`nix run .#figma\``);
  return Number(value);
};
const STACK = port("FIGMA_STACK_PORT");
const WEB = port("FIGMA_WEB_PORT");
if (WEB !== STACK + 1) throw new Error(`FIGMA_WEB_PORT ${WEB}: \`dev --port ${STACK}\` serves its next dev on ${STACK + 1}`);

export const SESSION = new URL("../../tmp/playwright/session.json", import.meta.url).pathname;
const STACK_TIMELINE = new URL(`../../../.local-port-${STACK}/timeline.tsv`, import.meta.url).pathname; // local-stack's state dir for `--port`

// The real stack on ports of its own (`dev --port`: its conductor, dev sign-in as an admin, and its
// `next dev`), never a running one's servers: those may face the real backend, as an operator. Run
// through the flake, which supplies `@playwright/test` with its pinned browsers.
export default defineConfig({
  testDir: ".",
  outputDir: "../../tmp/playwright",
  reporter: [["list"], ["./timeline.ts", { stack: STACK_TIMELINE, out: new URL("../../tmp/playwright/timeline.txt", import.meta.url).pathname }]],
  globalSetup: "./sign-in.ts",
  use: {
    baseURL: `http://localhost:${WEB}`, // `next dev` refuses its dev resources to any other host, so the page never hydrates
    deviceScaleFactor: 1,
    storageState: SESSION,
  },
  projects: [
    { name: "desktop", use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } } },
    { name: "mobile", use: { ...devices["Desktop Chrome"], viewport: { width: 390, height: 844 } } },
  ],
  webServer: {
    command: `dev --port ${STACK} --role admin --no-peers`, // the specs touch the conductor's own screens alone
    cwd: "../../..",
    url: `http://localhost:${WEB}/`,
    reuseExistingServer: false,
    timeout: 30 * 60_000, // the conductor's and its peers' builds
  },
});
