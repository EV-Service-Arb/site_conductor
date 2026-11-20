import { port, root, stackConfig } from "./playwright.config";

// The site as it is used: a copy of .local's data, the watched dev's peers, and this machine's Google accounts, whose
// browser key draws the map (`nix run .#figma`); the conductor serves its static export, built once from a copy of the checkout.
const panel = port("FIGMA_STACK_PORT");
export default stackConfig({
  stack: {
    argv: ["--port", String(panel), "--fixed", "--role", "admin"],
    env: {},
    state: `${root}.local-port-${panel}`,
    web: `http://localhost:${panel}`, // the sign-in sends the browser back to this origin
    health: `http://127.0.0.1:${panel}/health`,
    fresh: "fixed",
  },
  specs: { testMatch: "figma.spec.ts" },
});
