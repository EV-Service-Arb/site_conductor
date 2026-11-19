"use client";

import { Badge, Button, Card, CardContent, Field, FieldLabel, Input } from "@evinvest/uikit";
import { Store } from "lucide-react";
import { useId, useState } from "react";

import { api, ok } from "@/shared/api";
import type { Business, Gmail } from "@/shared/api/schema";
import { useLocale, useT, account } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { ConnectHelp, Copyable } from "@/shared/ui/connect-help";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const CONSOLE = "https://console.cloud.google.com";

/** A gmail's Business Profile: the OAuth client of a GCP project it made, and its grant to it; only that gmail's GBPs open through it. */
export function BusinessCard({ gmail, business, redirectUri, onChanged }: { gmail: Gmail; business: Business | null; redirectUri: string; onChanged: () => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const path = { params: { path: { id: gmail.id } } };
  const act = async (run: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await run();
      onChanged();
      setEditing(false);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  const grant = () =>
    void act(async () => {
      const { url } = await ok(api.POST("/gmails/{id}/business/consent", path));
      window.location.assign(url);
    });
  const status = business === null ? null : business.granted_at === null ? "pending" : business.checked?.error == null ? "works" : "failing";

  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-3 px-4">
        <header className="flex items-center gap-2">
          <Store aria-hidden className="size-4 text-ink-soft" />
          <h3 className="flex-1 text-sm font-semibold text-ink">{t(account.business.title, "Google Business Profile")}</h3>
          {status === "works" && <Badge tone="positive">{t(account.business.works, "Works")}</Badge>}
          {status === "failing" && <Badge tone="error">{t(account.business.failing, "Failing")}</Badge>}
          {status === "pending" && <Badge tone="warn">{t(account.business.pending, "Not granted")}</Badge>}
        </header>
        {business && !editing ? (
          <>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-ink-soft">{t(account.business.client, "Client")}</dt>
              <dd className="truncate text-right font-mono text-ink">{business.client_id.split("-")[0]}…</dd>
              <dt className="text-ink-soft">{t(account.business.set, "Set")}</dt>
              <dd className="text-right text-ink-soft">{formatDateTime(business.set_at, locale)}</dd>
              {business.granted_at !== null && (
                <>
                  <dt className="text-ink-soft">{t(account.business.granted, "Granted")}</dt>
                  <dd className="text-right text-ink-soft">{formatDateTime(business.granted_at, locale)}</dd>
                </>
              )}
            </dl>
            {business.checked && (
              <p className="text-xs text-ink-soft">
                {t(account.checkedAt, "Checked {at}", { at: formatDateTime(business.checked.at, locale) })}
                {business.checked.error !== null && <span className="block text-accent-error">{business.checked.error}</span>}
              </p>
            )}
            <div className="flex flex-wrap gap-2">
              <Button variant={business.granted_at === null ? "primary" : "outline"} size={button("sm")} disabled={busy} onClick={grant}>
                {business.granted_at === null ? t(account.business.grant, "Grant as {gmail}", { gmail: gmail.gmail }) : t(account.business.regrant, "Grant again")}
              </Button>
              {business.granted_at !== null && (
                <Button variant="outline" size={button("sm")} disabled={busy} onClick={() => void act(() => ok(api.POST("/gmails/{id}/business/test", path)))}>
                  {t(account.test, "Test")}
                </Button>
              )}
              <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => setEditing(true)}>
                {t(account.replace, "Replace")}
              </Button>
              <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => void act(() => ok(api.DELETE("/gmails/{id}/business", path)))}>
                {t(account.remove, "Remove")}
              </Button>
            </div>
          </>
        ) : (
          <BusinessForm busy={busy} onCancel={business ? () => setEditing(false) : undefined} onSubmit={(v) => void act(() => ok(api.PUT("/gmails/{id}/business", { ...path, body: v })))} />
        )}
        <BusinessHelp gmail={gmail.gmail} redirectUri={redirectUri} />
        <p className="text-xs text-ink-soft">
          {t(account.business.why, "Opens this gmail's own GBPs to the Business Profile APIs: categories as configured, owner's photos, reviews. Google answers 0 requests until it approves the project.")}
        </p>
      </CardContent>
    </Card>
  );
}

function BusinessHelp({ gmail, redirectUri }: { gmail: string; redirectUri: string }) {
  const t = useT();
  return (
    <ConnectHelp
      title={t(account.business.help.title, "Business Profile client, step by step")}
      video="/tutorials/business-profile.webm"
      steps={[
        { text: t(account.business.help.signIn, "In Chrome, sign in to Google as {gmail}.", { gmail }) },
        { text: t(account.business.help.project, "Open this link. Name: gbp. Press Create."), href: `${CONSOLE}/projectcreate` },
        { text: t(account.business.help.enableAccounts, "Open this link. Press Enable."), href: `${CONSOLE}/apis/library/mybusinessaccountmanagement.googleapis.com` },
        { text: t(account.business.help.enableInfo, "Open this link. Press Enable."), href: `${CONSOLE}/apis/library/mybusinessbusinessinformation.googleapis.com` },
        {
          text: t(account.business.help.branding, "Open this link. Press Get started. App name: gbp. Support email: {gmail}. Audience: External. Contact email: {gmail}. Press Create.", { gmail }),
          href: `${CONSOLE}/auth/branding`,
        },
        { text: t(account.business.help.testUser, "Open this link. Under Test users press Add users, type {gmail}, press Save.", { gmail }), href: `${CONSOLE}/auth/audience` },
        {
          text: (
            <>
              {t(account.business.help.client, "Open this link. Press Create client. Type: Web application. Under Authorized redirect URIs press Add URI and paste:")} <Copyable value={redirectUri} />{" "}
              {t(account.business.help.clientCreate, "Press Create.")}
            </>
          ),
          href: `${CONSOLE}/auth/clients/create`,
        },
        { text: t(account.business.help.paste, "Copy Client ID and Client secret from the window Google shows into the form above. Press Connect.") },
        { text: t(account.business.help.grant, "Press Grant as {gmail}. Pick {gmail}. Tick every box. Press Continue.", { gmail }) },
        { text: t(account.business.help.number, "Open this link. Copy Project number."), href: `${CONSOLE}/home/dashboard` },
        {
          text: t(account.business.help.apply, "Open this link. Pick Application for Basic API Access. Paste the project number. Send it from {gmail}. Wait for Google's email; then press Test.", { gmail }),
          href: "https://support.google.com/business/contact/api_default",
        },
      ]}
    />
  );
}

function BusinessForm({ busy, onCancel, onSubmit }: { busy: boolean; onCancel: (() => void) | undefined; onSubmit: (v: { client_id: string; client_secret: string }) => void }) {
  const t = useT();
  const id = useId();
  const button = useButtonSize();
  const size = useControlSize();
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const ready = clientId.trim().endsWith(".apps.googleusercontent.com") && secret.trim() !== "";
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) onSubmit({ client_id: clientId.trim(), client_secret: secret.trim() });
      }}
    >
      <Field>
        <FieldLabel htmlFor={`${id}-id`}>{t(account.business.clientId, "Client ID")}</FieldLabel>
        <Input id={`${id}-id`} size={size} value={clientId} onChange={(e) => setClientId(e.target.value)} placeholder="….apps.googleusercontent.com" autoComplete="off" />
      </Field>
      <Field>
        <FieldLabel htmlFor={`${id}-secret`}>{t(account.business.secret, "Client secret")}</FieldLabel>
        <Input id={`${id}-secret`} size={size} type="password" value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete="new-password" />
      </Field>
      <div className="flex gap-2">
        <Button type="submit" size={button("sm")} disabled={!ready || busy}>
          {t(account.business.connect, "Connect")}
        </Button>
        {onCancel && (
          <Button type="button" variant="ghost" size={button("sm")} disabled={busy} onClick={onCancel}>
            {t(account.cancel, "Cancel")}
          </Button>
        )}
      </div>
    </form>
  );
}
