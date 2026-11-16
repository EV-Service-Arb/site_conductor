import { grants } from "@/entities/session";
import { safeLocalPath } from "@/shared/api";
import { type Need, type Permission, needValues } from "@/shared/api/schema";

export const NEEDS = needValues;

/** The sign-in's `return_to` rule (`sa_server::signin`): a path on this origin, nothing a browser reads as another. */
export const safeContinue = safeLocalPath;

export function holds(permissions: readonly Permission[], need: Need): boolean {
  return grants(need).every((p) => permissions.includes(p));
}
