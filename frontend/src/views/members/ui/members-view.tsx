"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Card, InputGroup, InputGroupAddon, InputGroupInput, PageFrame, Settled, Skeleton } from "@evinvest/uikit";
import { Search } from "lucide-react";
import { useEffect, useState } from "react";

import { session, stopActing, useMe } from "@/entities/session";
import { api, ok } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { ErrorState } from "@/shared/ui/error-state";

/** Everyone, to view the panel as; while acting, opening it is the way back. */
export function MembersView() {
  const t = useT();
  const me = useMe();
  const members = useResource(api, "/members");
  const [query, setQuery] = useState("");

  const acting = me.acting !== null;
  useEffect(() => {
    if (acting) void stopActing();
  }, [acting]);
  if (acting) return null;

  const q = query.trim().toLowerCase();
  return (
    <PageFrame breakpoint="md" title={t(session.members.title, "View as member")} description={t(session.members.description, "Their account, as they see it, with your permissions. Their payments and sign-ins stay theirs.")}>
      <Settled loading={members.status === "loading"} skeleton={<Skeleton className="h-40 w-full" />}>
        {members.status === "error" && <ErrorState failure={members.failure} onRetry={members.reload} />}
        {members.status === "ok" && (
          <Card className="gap-0 overflow-hidden py-0">
            <div className="border-b border-border p-3">
              <InputGroup>
                <InputGroupAddon>
                  <Search aria-hidden />
                </InputGroupAddon>
                <InputGroupInput type="search" autoFocus value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t(session.members.search, "Name or email")} aria-label={t(session.members.search, "Name or email")} />
              </InputGroup>
            </div>
            <ul className="divide-y divide-border">
              {members.data.users
                .filter((u) => u.user_id !== me.user_id && (u.name.toLowerCase().includes(q) || u.email.toLowerCase().includes(q)))
                .map((u) => (
                  <li key={u.user_id} className="flex items-center justify-between gap-3 px-3 py-2">
                    <span className="min-w-0">
                      <span className="block truncate text-sm font-medium text-ink">{u.name || u.email}</span>
                      {u.name && <span className="block truncate text-xs text-ink-soft">{u.email}</span>}
                    </span>
                    <Button type="button" variant="outline" size="sm" onClick={() => void actAs(u.user_id)}>
                      {t(session.members.view, "View as")}
                    </Button>
                  </li>
                ))}
            </ul>
          </Card>
        )}
      </Settled>
    </PageFrame>
  );
}

async function actAs(user_id: string) {
  await ok(api.PUT("/me/acting", { body: { user_id } }));
  location.assign(location.origin); // the live socket, caches and the map's session are the identity's
}
