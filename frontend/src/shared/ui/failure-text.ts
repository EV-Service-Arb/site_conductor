import type { ApiFailure } from "@/shared/api";
import type { T } from "@/shared/i18n";

/** What to tell a person about a failed call; the backend's own words where it gave some. */
export function failureText(failure: ApiFailure | { kind: "invalid"; message: string }, t: T): string {
  switch (failure.kind) {
    case "csrf":
      return t("The page is out of date. Reload it and repeat the action.");
    case "forbidden":
      return t("Your role may not do this.");
    case "unavailable":
      return t("Try again in a minute. You are still signed in.");
    case "no_access":
      return t("No access to the Service-Arb panel");
    case "bad_request":
    case "conflict":
      return failure.message;
    case "invalid_fields":
      return Object.values(failure.fields).join(" · ") || t("Could not load this. {detail}", { detail: "422" });
    case "invalid_path":
      return `${failure.path}: ${failure.message}`;
    case "failed":
      return t("Could not load this. {detail}", { detail: `HTTP ${failure.status}` });
    case "invalid":
      return t("Could not load this. {detail}", { detail: failure.message });
    default:
      return t("Could not load this. {detail}", { detail: failure.kind });
  }
}
