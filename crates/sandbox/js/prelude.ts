// The client a run's code calls the operations through: the catalog's paths, absolute, as `search` names them.
import { createApi } from "@evinvest/api";
import catalog from "./catalog.json" with { type: "json" };
import type { paths } from "./catalog.ts";

const token = Deno.env.get("SA_RUN_TOKEN")!;

export const api = createApi<paths>({
  document: catalog,
  baseUrl: Deno.env.get("SA_RUN_URL")!,
  headers: () => ({ authorization: `Bearer ${token}` }),
  onUnauthenticated: () => {
    console.error("the caller's session ended: the run stops here");
    Deno.exit(3);
  },
  // the conductor's own refusal (sa_sandbox::REFUSED), not the operation's: nothing after it runs
  fetch: async (input, init) => {
    const answer = await fetch(input, init);
    if (answer.headers.has("x-sa-run-refused")) {
      console.error(await answer.text());
      Deno.exit(4);
    }
    return answer;
  },
});
