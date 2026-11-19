import { stackConfig } from "./playwright.config";

// The site as it is used: prod's data and every peer whose bundle the front end mounts (`nix run .#figma`).
export default stackConfig({ ports: ["FIGMA_STACK_PORT", "FIGMA_WEB_PORT"], args: "--reset --from-prod", specs: { testMatch: "figma.spec.ts" } });
