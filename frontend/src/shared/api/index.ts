import { type Network, createApi } from "@evinvest/api";

import { readCsrf } from "./csrf";
import openapi from "./openapi.json";
import type { paths } from "./schema";
import reviewArchiveDocument from "./services/review_archive.json";
import sitesDocument from "./services/sites.json";
import type { paths as SitesPaths } from "./services/sites";
import type { paths as ReviewArchivePaths } from "./services/review_archive";
import { goToSignIn } from "./signin-overlay";

/** The double-submit token on every write; a signed-in page always has its cookie. */
function csrf(method: string): Record<string, string> {
  if (method === "GET") return {};
  const token = readCsrf(document.cookie);
  if (token === null) throw new Error("a write with no CSRF cookie beside the session");
  return { "x-sa-csrf": token };
}

export const api = createApi<paths>({ document: openapi, baseUrl: "/api/v1", headers: csrf, onUnauthenticated: goToSignIn });

// A forwarded service's 401 is not the panel's session ending: the panel's own reads see to that.
const theirs = () => {};
export const reviewArchive = createApi<ReviewArchivePaths>({ document: reviewArchiveDocument, baseUrl: "/api/review_archive", headers: csrf, onUnauthenticated: theirs });
export const sites = createApi<SitesPaths>({ document: sitesDocument, baseUrl: "/api/sites", headers: csrf, onUnauthenticated: theirs });

/** A refusal or no answer, as every service words one: `code` from the operation's `x-errors`, or `network`. */
export type Failure = { code: string; error: string } | Network;

/** A call's refusal, thrown by `ok` for a flow that only reports it. */
export class Refused extends Error {
  override name = "Refused";
  constructor(readonly failure: Failure) {
    super(`refused: ${failure.code}`);
  }
}

type Data<A> = A extends { data: infer D; error?: never } ? D : never;

/** The answer's data; a refusal thrown as `Refused`, for a flow whose every failure is told the same way. */
export async function ok<A extends { data?: unknown; error?: unknown }>(call: Promise<A>): Promise<Data<A>> {
  const answer = await call;
  if (answer.error !== undefined) throw new Refused(answer.error as Failure);
  return answer.data as Data<A>;
}

export { dismissOverlay, goToSignIn, openOverlay, overlayState, redirectToSignIn, registerOverlayHost, safeLocalPath, SIGN_IN_PATH, subscribeOverlay, switchAccountPath, takeReturnPage } from "./signin-overlay";
export { csrfCookieName, readCsrf } from "./csrf";
export { platformMfe } from "./platform-mfe";
