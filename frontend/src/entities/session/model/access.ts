import openapi from "@/shared/api/openapi.json";
import { type Caller, type Need, type Permission, permissionValues } from "@/shared/api/schema";

const members: Record<Exclude<Need, Permission>, readonly string[]> = openapi.components.schemas.Need["x-members"];

const isPermission = (need: Need): need is Permission => permissionValues.some((p) => p === need);

function asPermission(p: string): Permission {
  const found = permissionValues.find((q) => q === p);
  if (found === undefined) throw new Error(`the document's alias names ${p}, no permission of it`);
  return found;
}

/** The permissions `need` stands for: itself, or an alias's members as the document's `Need` names them. */
export function grants(need: Need): readonly Permission[] {
  return isPermission(need) ? [need] : members[need].map(asPermission);
}

/** Whether the caller holds `permission`: the set concierge resolved is concrete. */
export function may(caller: Caller, permission: Permission): boolean {
  return caller.permissions.includes(permission);
}

/** The sites nav lists the Work and Analysis sections: anything else it would show empty. */
export function seesSites(caller: Caller): boolean {
  return may(caller, "sa:sites:work:read") || may(caller, "sa:sites:analysis:read");
}

/** The caller's own screens, beside the account chip, left to right. `walletPage`: the build flag. */
export function ownScreens(caller: Caller, walletPage: boolean): ("usage" | "tokens" | "wallet")[] {
  return [
    "usage" as const,
    "tokens" as const,
    ...(walletPage && may(caller, "sa:sites:work:read") ? (["wallet"] as const) : []),
  ];
}
