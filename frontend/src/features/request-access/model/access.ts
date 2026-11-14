import { ALIASES } from "@/entities/session";
import { safeLocalPath } from "@/shared/api";
import { type Need, type Permission, needValues } from "@/shared/api/schema";

export const NEEDS = needValues;

export const PLAYBOOK_NEED: Need = "sa:playbook:mcp:use";

/** The sign-in's `return_to` rule (`sa_server::signin`): a path on this origin, nothing a browser reads as another. */
export const safeContinue = safeLocalPath;

export function holds(permissions: readonly Permission[], need: Need): boolean {
  const members: readonly Permission[] = need in ALIASES ? ALIASES[need as keyof typeof ALIASES] : [need as Permission];
  return members.every((p) => permissions.includes(p));
}
