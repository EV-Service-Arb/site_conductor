"use client";

import { useResource } from "@evinvest/api/react";
import { PageFrame, Settled, Skeleton } from "@evinvest/uikit";
import { mayOpen } from "@evinvest/views";
import { useState } from "react";

import { useMe } from "@/entities/session";
import { joinConnectedSites, sources as sourcesKeys } from "@/entities/source";
import { AddSourceForm, SecretDialog } from "@/features/manage-sources";
import { api } from "@/shared/api";
import type { NewSourceKey } from "@/shared/api/schema";
import { VIEWS } from "@/shared/api/views";
import { useT } from "@/shared/i18n";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";

import { ConnectedSitesTable } from "./connected-sites-table";
import { SourcesTable } from "./sources-table";
import { connected as connectedKeys } from "../model/i18n";

/** Admin only (spec §5.4): the signing keys the sources ingest with. */
export function SourcesView() {
  const t = useT();
  const me = useMe();
  const sources = useResource(api, "/sources");
  const connected = useResource(api, "/sources/sites");
  const joined = connected.status === "ok" ? joinConnectedSites(connected.data) : null;
  const [added, setAdded] = useState<NewSourceKey | null>(null);

  if (!mayOpen(VIEWS, "sources", me.permissions)) {
    return (
      <PageFrame breakpoint="md" title={t(sourcesKeys.title, "Sources")}>
        <ErrorState failure={{ code: "forbidden", error: "" }} />
      </PageFrame>
    );
  }

  return (
    <PageFrame breakpoint="md" title={t(sourcesKeys.title, "Sources")}>
      <AddSourceForm
        onAdded={(a) => {
          setAdded(a);
          sources.reload();
        }}
      />
      <Settled loading={sources.status === "loading"} skeleton={<Skeleton className="h-40 w-full" />}>
        {sources.status === "error" && <ErrorState failure={sources.failure} onRetry={sources.reload} />}
        {sources.status === "ok" && sources.data.sources.length === 0 && <EmptyState title={t(sourcesKeys.empty, "No sources yet")} description={t(sourcesKeys.empty.body, "Add a key for each site or import that sends events to the panel.")} />}
        {sources.status === "ok" && sources.data.sources.length > 0 && (
          <SourcesTable sources={sources.data.sources} onRevoked={sources.reload} />
        )}
      </Settled>
      <h2 className="text-lg font-semibold">{t(connectedKeys.title, "Connected sites")}</h2>
      <Settled loading={connected.status === "loading"} skeleton={<Skeleton className="h-24 w-full" />}>
        {connected.status === "error" && <ErrorState failure={connected.failure} onRetry={connected.reload} />}
        {joined !== null &&
          joined.sitesFailures.map((detail) => (
            <p key={detail} className="text-sm text-ink-soft">
              {t(connectedKeys.sitesFailed, "sites_panel did not answer: {detail}", { detail })}
            </p>
          ))}
        {joined !== null && joined.sites.length === 0 && <EmptyState title={t(connectedKeys.empty, "No site keys")} description={t(connectedKeys.empty.body, "A site key with its origin shows here, with what its site reports.")} />}
        {joined !== null && joined.sites.length > 0 && <ConnectedSitesTable sites={joined.sites} />}
      </Settled>
      <SecretDialog added={added} onClose={() => setAdded(null)} />
    </PageFrame>
  );
}
