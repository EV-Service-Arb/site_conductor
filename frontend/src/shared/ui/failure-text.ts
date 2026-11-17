import type { Failure } from "@/shared/api";
import type { T } from "@/shared/i18n";
import { state } from "@/shared/i18n";

/** What to tell a person about a failed call: words of the panel's own for what it acts on, else the service's. */
export function failureText(failure: Failure, t: T): string {
  switch (failure.code) {
    case "csrf":
      return t(state.csrf, "The page is out of date. Reload it and repeat the action.");
    case "forbidden":
      return t(state.forbidden, "Your role may not do this.");
    case "unavailable":
    case "network":
      return t(state.unavailable.body, "Try again in a minute. You are still signed in.");
    default:
      return failure.error === "" ? t(state.error, "Could not load this. {detail}", { detail: failure.code }) : failure.error;
  }
}
