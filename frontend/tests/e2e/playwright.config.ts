import { defineConfig, devices } from "@playwright/test";

const port = (name: string): number => {
  const value = process.env[name];
  if (!value) throw new Error(`${name}: the flake's localEnv sets it; run through \`nix run .#e2e\` or \`nix run .#figma\``);
  return Number(value);
};

// The real stack on ports of its own (`dev --port`: its conductor, dev sign-in as an admin, and its
// `next dev`), never a running one's servers: those may face the real backend, as an operator. Run
// through the flake, which supplies `@playwright/test` with its pinned browsers.
export function stackConfig({ ports, args, specs }: { ports: [stack: string, web: string]; args: string; specs: { testMatch: string } | { testIgnore: string } }) {
  const STACK = port(ports[0]);
  const WEB = port(ports[1]);
  if (WEB !== STACK + 1) throw new Error(`${ports[1]} ${WEB}: \`dev --port ${STACK}\` serves its next dev on ${STACK + 1}`);
  const tmp = (path: string) => new URL(`../../tmp/playwright-${STACK}/${path}`, import.meta.url).pathname;
  process.env.STACK_TIMELINE = new URL(`../../../.local-port-${STACK}/timeline.tsv`, import.meta.url).pathname; // where `dev --port` keeps it; read by timeline.ts, written by sign-in.ts too
  return defineConfig({
    testDir: ".",
    ...specs,
    fullyParallel: true, // the specs share no server state: each page is its own context, over the one session
    outputDir: tmp(""),
    reporter: [["list"], ["./timeline.ts", { out: tmp("timeline.txt") }]],
    globalSetup: "./sign-in.ts",
    use: {
      baseURL: `http://localhost:${WEB}`, // `next dev` refuses its dev resources to any other host, so the page never hydrates
      deviceScaleFactor: 1,
      storageState: tmp("session.json"),
    },
    projects: [
      { name: "desktop", use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } } },
      { name: "mobile", use: { ...devices["Desktop Chrome"], viewport: { width: 390, height: 844 } } },
    ],
    webServer: {
      command: `dev --port ${STACK} --role admin ${args}`,
      cwd: "../../..",
      url: `http://localhost:${WEB}/`,
      reuseExistingServer: false,
      env: { SA_CONDUCTOR_CONFIG: new URL("./conductor.nix", import.meta.url).pathname }, // no Google accounts: never spends on, nor reads, a person's
      timeout: 30 * 60_000, // the conductor's and its peers' builds; with --from-prod, the restore too
    },
  });
}

// the specs touch the conductor's own screens alone
export default stackConfig({ ports: ["E2E_STACK_PORT", "E2E_WEB_PORT"], args: "--no-peers", specs: { testIgnore: "figma.spec.ts" } });
