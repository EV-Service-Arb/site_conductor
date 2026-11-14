import type { Caller, Need, Permission } from "@/shared/api/schema";

/** What each alias of the `sa` catalog grants (`sa_auth`): the document names the aliases, not their members. */
export const ALIASES = {
  "sa:admin": ["sa:admin:sources:manage", "sa:admin:tokens:grant", "sa:admin:trash:keep", "sa:placement:study:read", "sa:placement:study:spend", "sa:playbook:mcp:use", "sa:review_archive:archive:operate", "sa:review_archive:members:act_as", "sa:sites:analysis:experiments:edit", "sa:sites:analysis:read", "sa:sites:work:invoices:create", "sa:sites:work:leads:edit", "sa:sites:work:pii:see", "sa:sites:work:places:edit", "sa:sites:work:pricing:edit", "sa:sites:work:read"],
  "sa:everyone": ["sa:playbook:mcp:use"],
  "sa:operator": ["sa:placement:study:read", "sa:sites:analysis:read", "sa:sites:work:leads:edit", "sa:sites:work:pii:see", "sa:sites:work:read"],
} as const satisfies Record<Exclude<Need, Permission>, readonly Permission[]>;

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
