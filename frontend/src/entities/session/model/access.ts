import { mayOpen } from "@evinvest/views";

import openapi from "@/shared/api/openapi.json";
import { VIEWS } from "@/shared/api/views";
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

/** Whether one of sites' views that names a permission opens for the caller: its start names none, and shows nothing alone. */
export function seesSites(caller: Caller): boolean {
  return (Object.keys(VIEWS.views) as (keyof typeof VIEWS.views)[]).some((view) => view.startsWith("sites.") && "needs" in VIEWS.views[view] && mayOpen(VIEWS, view, caller.permissions));
}
