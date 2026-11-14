"use client";

import { Badge, Button, Card, CardContent, Field, FieldLabel, Input } from "@evinvest/uikit";
import { Mail } from "lucide-react";
import { useId, useState } from "react";

import { type Gmail, type Imap, disconnect, setImap, testConnection } from "@/entities/gmb";
import { useLocale, useT, account } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

/** A gmail's inbox: set once it logs in, the password never shown again. */
export function ImapCard({ gmail, imap, onChanged }: { gmail: Gmail; imap: Imap | null; onChanged: () => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
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

  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-3 px-4">
        <header className="flex items-center gap-2">
          <Mail aria-hidden className="size-4 text-ink-soft" />
          <h3 className="flex-1 text-sm font-semibold text-ink">{t(account.imap.title, "Email · IMAP")}</h3>
          {imap && <Badge tone={imap.checked.error === null ? "positive" : "error"}>{(imap.checked.error === null ? t(account.imap.connected, "Connected") : t(account.imap.failing, "Failing"))}</Badge>}
        </header>
        {imap && !editing ? (
          <>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-ink-soft">{t(account.imap.host, "Server")}</dt>
              <dd className="text-right text-ink">{imap.host}</dd>
              <dt className="text-ink-soft">{t(account.imap.username, "Login")}</dt>
              <dd className="truncate text-right text-ink">{imap.username}</dd>
              <dt className="text-ink-soft">{t(account.imap.password, "App password")}</dt>
              <dd className="text-right text-ink-soft">{t(account.setAt, "set {at}", { at: formatDateTime(imap.set_at, locale) })}</dd>
            </dl>
            <p className="text-xs text-ink-soft">
              {t(account.checkedAt, "Checked {at}", { at: formatDateTime(imap.checked.at, locale) })}
              {imap.checked.error !== null && <span className="block text-accent-error">{imap.checked.error}</span>}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" size={button("sm")} disabled={busy} onClick={() => void act(() => testConnection(gmail.id, "imap"))}>
                {t(account.test, "Test")}
              </Button>
              <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => setEditing(true)}>
                {t(account.replace, "Replace")}
              </Button>
              <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() =>
                  void act(() => disconnect(gmail.id, "imap"))
                }>
                {t(account.remove, "Remove")}
              </Button>
            </div>
          </>
        ) : (
          <ImapForm gmail={gmail} busy={busy} onCancel={imap ? () => setEditing(false) : undefined} onSubmit={(v) => void act(() => setImap(gmail.id, v))} />
        )}
        <p className="text-xs text-ink-soft">{t(account.imap.why, "Inbox alerts and email automations will read it; nothing reads mail yet.")}</p>
      </CardContent>
    </Card>
  );
}

function ImapForm({
  gmail,
  busy,
  onCancel,
  onSubmit,
}: {
  gmail: Gmail;
  busy: boolean;
  onCancel: (() => void) | undefined;
  onSubmit: (v: { host: string; username: string; password: string }) => void;
}) {
  const t = useT();
  const id = useId();
  const button = useButtonSize();
  const size = useControlSize();
  // what a Gmail or Workspace inbox answers on; another provider's is typed over it
  const [host, setHost] = useState("imap.gmail.com");
  const [username, setUsername] = useState(gmail.gmail);
  const [password, setPassword] = useState("");
  const ready = host.trim() !== "" && username.trim() !== "" && password !== "";
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) onSubmit({ host: host.trim(), username: username.trim(), password });
      }}
    >
      <Field>
        <FieldLabel htmlFor={`${id}-host`}>{t(account.imap.host, "Server")}</FieldLabel>
        <Input id={`${id}-host`} size={size} value={host} onChange={(e) => setHost(e.target.value)} autoComplete="off" />
      </Field>
      <Field>
        <FieldLabel htmlFor={`${id}-user`}>{t(account.imap.username, "Login")}</FieldLabel>
        <Input id={`${id}-user`} size={size} value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="off" />
      </Field>
      <Field>
        <FieldLabel htmlFor={`${id}-pass`}>{t(account.imap.appPassword, "App password")}</FieldLabel>
        <Input id={`${id}-pass`} size={size} type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" />
      </Field>
      <p className="text-xs text-ink-soft">{t(account.imap.how, "Google account → Security → 2-step verification → App passwords. Checked by logging in before it is kept; never shown again.")}</p>
      <div className="flex gap-2">
        <Button type="submit" size={button("sm")} disabled={!ready || busy}>
          {t(account.imap.connect, "Connect")}
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
