import { port, root, stackConfig } from "./playwright.config";

// The site as it is used: prod's data, every peer whose bundle the front end mounts, and this machine's Google
// accounts, whose browser key draws the map (`nix run .#figma`). Every port moved, so its peers are its own, on prod's
// data; the conductor serves its static export, built once from a copy of the checkout. `--restart` brings prod's data anew.
const offset = port("FIGMA_PORT_OFFSET");
const panel = port("PANEL_BIND") + offset;
export default stackConfig({
  stack: {
    argv: ["--port-offset", String(offset), "--fixed", "--from-prod", "--role", "admin"],
    env: {},
    state: `${root}.local-${offset}`,
    web: `http://localhost:${panel}`, // the sign-in sends the browser back to this origin
    health: `http://127.0.0.1:${panel}/health`,
    fresh: "fixed",
  },
  specs: { testMatch: "figma.spec.ts" },
});
