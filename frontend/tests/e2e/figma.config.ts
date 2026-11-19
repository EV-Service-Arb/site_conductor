import { stackConfig } from "./playwright.config";

// The site as it is used: prod's data, every peer whose bundle the front end mounts, and this machine's Google
// accounts, whose browser key draws the map (`nix run .#figma`).
export default stackConfig({ ports: ["FIGMA_STACK_PORT", "FIGMA_WEB_PORT"], args: "--reset --from-prod", specs: { testMatch: "figma.spec.ts" }, env: {}, persistent: false }); // --from-prod and the peers: not left running
