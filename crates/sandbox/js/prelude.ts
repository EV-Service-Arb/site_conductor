// The client a cell's code calls the operations through: the catalog's paths, absolute, as `search` names them.
import { createApi } from "@evinvest/api";
import catalog from "./catalog.json" with { type: "json" };
import type { paths } from "./catalog.ts";

/** The running cell's token: the host sets it before each cell. */
let token = "";
export function setToken(t: string): void {
  token = t;
}

export const api = createApi<paths, "refused">({
  document: catalog,
  baseUrl: Deno.env.get("SA_RUN_URL")!,
  headers: () => ({ authorization: `Bearer ${token}` }),
  onUnauthenticated: () => {
    console.error("the caller's session ended: the session stops here, its state with it");
    Deno.exit(3);
  },
  // the conductor's own refusal (sa_sandbox::REFUSED): a call not let by, declined, or past a cell's calls
  hop: { refused: 403 },
});
