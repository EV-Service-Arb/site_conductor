import Ajv2020, { type ValidateFunction } from "ajv/dist/2020";
import createClient, { type Client } from "openapi-fetch";

import { CSRF_HEADER, readCsrf } from "./csrf";
import { ApiError, type HttpDeps, failureOf, goToSignIn } from "./http";
import openapi from "./openapi.json";
import type { paths } from "./schema";

/** A cast would only promise the shape: every answer is checked against its operation's schema in the same document the types come from. */
const ajv = new Ajv2020({ strict: false, validateFormats: false, validateSchema: false });
ajv.addSchema(openapi, "api");

/** `null`: an answer the document declares opaque, a JSON body with no schema (placement's payload). */
function answerSchema(path: string, method: string, status: number): ValidateFunction | null {
  const media = (openapi.paths as Record<string, Record<string, { responses: Record<string, { content?: Record<string, { schema?: unknown }> }> }>>)[path]?.[method.toLowerCase()]?.responses[String(status)]?.content?.["application/json"];
  if (media !== undefined && media.schema === undefined) return null;
  const pointer = ["paths", path, method.toLowerCase(), "responses", String(status), "content", "application/json", "schema"]
    .map((p) => encodeURIComponent(p.replaceAll("~", "~0").replaceAll("/", "~1")))
    .join("/");
  const validate = ajv.getSchema(`api#/${pointer}`);
  if (!validate) throw new Error(`${method} ${path}: the document has no JSON answer for ${status}`);
  return validate;
}

type Api = Client<paths>;

/** `/api/v1` as its document describes it; a call throws an `ApiError` for any failure, so its answer is always `data`. */
export function createApi(deps: HttpDeps, baseUrl: string): Api {
  const client = createClient<paths>({ baseUrl, fetch: deps.fetch, credentials: "same-origin", cache: "no-store", headers: { accept: "application/json" } });
  client.use({
    onRequest({ request }) {
      if (request.method === "GET") return;
      const csrf = readCsrf(deps.cookie());
      if (csrf) request.headers.set(CSRF_HEADER, csrf);
    },
    async onResponse({ request, response, schemaPath }) {
      if (!response.ok) {
        const failure = await failureOf(response);
        if (failure.kind === "unauthenticated") deps.onUnauthenticated();
        throw new ApiError(failure);
      }
      if (response.status === 204) return;
      const validate = answerSchema(schemaPath, request.method, response.status);
      if (validate === null) return;
      const body: unknown = await response.clone().json();
      if (!validate(body)) throw new Error(`${request.method} ${schemaPath}: ${ajv.errorsText(validate.errors)}`);
    },
    onError: () => new ApiError({ kind: "network" }),
  });
  return client;
}

export const api: Api = createApi(
  {
    fetch: (input, init) => fetch(input, init),
    cookie: () => (typeof document === "undefined" ? "" : document.cookie),
    onUnauthenticated: goToSignIn,
  },
  "/api/v1",
);

/** A call's answer: the middleware has thrown on every failure. */
export async function ok<T>(call: Promise<{ data?: T; response: Response }>): Promise<T> {
  const { data, response } = await call;
  if (!response.ok) throw new Error("unreachable: the client throws on a failed answer");
  return data as T;
}
