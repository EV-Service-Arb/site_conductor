"use client";

import { Button, Checkbox, Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger, Field, FieldDescription, FieldLabel, Input, ToggleGroup, ToggleGroupItem, toast } from "@evinvest/uikit";
import { Copy, Plus } from "lucide-react";
import { type ReactNode, useId, useState } from "react";

import { METHOD_ICON, alerts as keys, pushable, useAlerts } from "@/entities/alerts";
import { useMe } from "@/entities/session";
import { api, ok } from "@/shared/api";
import type { Alerts, Method } from "@/shared/api/schema";
import { type T, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { ConnectTelegram, PushHere } from "./connect";

const METHODS = ["telegram", "email", "push", "discord", "slack", "webhook"] as const satisfies readonly Method[];

const OFFER: Record<Method, { name: (t: T) => string; what: (t: T) => string }> = {
  telegram: { name: () => "Telegram", what: (t) => t(keys.add.telegram, "your chat, a channel, group or topic") },
  email: { name: (t) => t(keys.method.email, "Email"), what: (t) => t(keys.add.email, "your account's address, or another") },
  push: { name: (t) => t(keys.push.here, "Push on this browser"), what: (t) => t(keys.add.push, "this phone or computer, panel closed or not") },
  discord: { name: () => "Discord", what: (t) => t(keys.add.discord, "a channel") },
  slack: { name: () => "Slack", what: (t) => t(keys.add.slack, "a channel") },
  webhook: { name: () => "Webhook", what: (t) => t(keys.add.webhook, "signed JSON to your https address") },
};

/** + Add: the methods this server has, each one's step a dialog with one field or one button. */
export function AddDestination({ className }: { className?: string }) {
  const t = useT();
  const me = useMe();
  const size = useButtonSize();
  const alerts = useAlerts();
  const [adding, setAdding] = useState<Method | null>(null);
  if (alerts.status !== "ok" || !api.mayCall("post", "/alerts/email", me.permissions)) return null;
  const { data } = alerts;
  const offered = METHODS.filter((k) => data.methods.includes(k) && (k !== "push" || (pushable() !== "no" && me.acting === null)));
  const done = () => {
    setAdding(null);
    alerts.reload();
  };
  if (offered.length === 0) return <p className="text-sm text-ink-soft">{t(keys.off, "Nothing is set up on this server to reach you by.")}</p>;
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button type="button" variant="ghost" size={size("sm")} className={className}>
            <Plus aria-hidden className="size-4" />
            {t(keys.add, "Add")}
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-80">
          {offered.map((k) => {
            const Icon = METHOD_ICON[k];
            return (
              <DropdownMenuItem key={k} onClick={() => setAdding(k)} className="gap-3 py-2">
                <Icon className="size-4 shrink-0" />
                <span className="flex min-w-0 flex-col">
                  <span className="text-ink">{OFFER[k].name(t)}</span>
                  <span className="text-xs text-ink-soft">{OFFER[k].what(t)}</span>
                </span>
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuContent>
      </DropdownMenu>
      <Dialog open={adding !== null} onOpenChange={(open) => !open && setAdding(null)}>
        {adding !== null && (
          <DialogContent className="max-w-md">
            <Step kind={adding} alerts={data} onDone={done} />
          </DialogContent>
        )}
      </Dialog>
    </>
  );
}

function Step({ kind, alerts, onDone }: { kind: Method; alerts: Alerts; onDone: () => void }) {
  switch (kind) {
    case "telegram":
      return <Telegram alerts={alerts} onDone={onDone} />;
    case "email":
      return <Email alerts={alerts} onDone={onDone} />;
    case "push":
      return <Push onDone={onDone} />;
    case "discord":
      return <Discord onDone={onDone} />;
    case "slack":
      return <Slack onDone={onDone} />;
    case "webhook":
      return <Webhook onDone={onDone} />;
  }
}

function Head({ title, reads }: { title: string; reads: "names" | "no_names" }) {
  const t = useT();
  return (
    <DialogHeader>
      <DialogTitle>{title}</DialogTitle>
      <DialogDescription>{reads === "names" ? t(keys.add.names, "Only you read it: names, phones and buttons come with it.") : t(keys.add.noNames, "Whoever is there reads it: no names, phones or buttons.")}</DialogDescription>
    </DialogHeader>
  );
}

/** One address, and what becomes of it. */
function Address({ label, placeholder, hint, submit, start = "", type = "text", children, send }: { label: string; placeholder: string; hint: ReactNode; submit: string; start?: string; type?: "text" | "email" | "url"; children?: ReactNode; send: (address: string) => Promise<void> }) {
  const t = useT();
  const id = useId();
  const control = useControlSize();
  const size = useButtonSize();
  const [address, setAddress] = useState(start);
  const [busy, setBusy] = useState(false);
  const go = async () => {
    setBusy(true);
    try {
      await send(address.trim());
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      className="grid gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        void go();
      }}
    >
      <Field className="flex flex-col gap-1">
        <FieldLabel htmlFor={id}>{label}</FieldLabel>
        <Input id={id} size={control} type={type} required autoCapitalize="none" value={address} placeholder={placeholder} onChange={(e) => setAddress(e.target.value)} />
        <FieldDescription>{hint}</FieldDescription>
      </Field>
      {children}
      <Button type="submit" size={size("sm")} disabled={busy || address.trim() === ""} className="justify-self-start">
        {submit}
      </Button>
    </form>
  );
}

function Either<V extends string>({ value, onChange, options, label }: { value: V; onChange: (v: V) => void; options: readonly { value: V; label: string }[]; label: string }) {
  return (
    <ToggleGroup type="single" variant="outline" size="sm" value={value} onValueChange={(v) => v !== "" && onChange(v as V)} aria-label={label} className="justify-self-start">
      {options.map((o) => (
        <ToggleGroupItem key={o.value} value={o.value}>
          {o.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}

function Telegram({ alerts, onDone }: { alerts: Alerts; onDone: () => void }) {
  const t = useT();
  const own = alerts.destinations.some((d) => d.kind === "private");
  const [what, setWhat] = useState<"own" | "chat">(own ? "chat" : "own");
  return (
    <>
      <Head title="Telegram" reads={what === "own" ? "names" : "no_names"} />
      <Either
        label={t(keys.add.where, "Where")}
        value={what}
        onChange={setWhat}
        options={[
          { value: "own", label: t(keys.add.telegram.own, "Your chat") },
          { value: "chat", label: t(keys.add.telegram.chat, "Channel, group or topic") },
        ]}
      />
      {what === "own" ? (
        <div className="grid gap-3">
          <p className="text-sm text-ink-soft">{t(keys.add.telegram.how, "Opens the bot: press Start there. It shows up here.")}</p>
          <div>
            <ConnectTelegram label={t(keys.add.telegram.open, "Open the bot")} onDone={onDone} />
          </div>
        </div>
      ) : !own ? (
        <p className="text-sm text-ink-soft">{t(keys.add.telegram.first, "Connect your own chat first: the bot checks with it that you run the channel.")}</p>
      ) : (
        <Address
          label={t(keys.add.telegram.chat, "Channel, group or topic")}
          placeholder="@channel"
          hint={t(keys.add.telegram.hint, "Make {bot} an admin there first, then its @name, its -100… id, or <group id>/<topic id>. Add sends a test.", { bot: alerts.bot ?? "…" })}
          submit={t(keys.add, "Add")}
          send={(chat) => ok(api.POST("/alerts/telegram/chats", { body: { chat } })).then(onDone)}
        />
      )}
    </>
  );
}

function Email({ alerts, onDone }: { alerts: Alerts; onDone: () => void }) {
  const t = useT();
  // the account's own is offered until it is there
  const own = alerts.email !== null && !alerts.destinations.some((d) => d.method === "email" && d.title === alerts.email) ? alerts.email : "";
  const send = async (address: string) => {
    await ok(api.POST("/alerts/email", { body: { address } }));
    if (address.toLowerCase() !== alerts.email?.toLowerCase()) toast.info(t(keys.add.email.sent, "A link is on its way to {address}: alerts start once it is opened.", { address }));
    onDone();
  };
  return (
    <>
      <Head title={t(keys.method.email, "Email")} reads="names" />
      <Address
        label={t(keys.add.email.address, "Address")}
        type="email"
        placeholder="name@company.com"
        start={own}
        hint={t(keys.add.email.hint, "Your account's address starts at once. Any other is mailed a link: open it signed in as yourself.")}
        submit={t(keys.add, "Add")}
        send={send}
      />
    </>
  );
}

function Push({ onDone }: { onDone: () => void }) {
  const t = useT();
  return (
    <>
      <Head title={t(keys.push.here, "Push on this browser")} reads="names" />
      {pushable() === "install" ? (
        <p className="text-sm text-ink-soft">{t(keys.push.install, "On an iPhone or iPad, add the panel to the Home Screen first: Share, then Add to Home Screen. Open it from there and come back here.")}</p>
      ) : (
        <div className="grid gap-3">
          <p className="text-sm text-ink-soft">{t(keys.push.how, "Allow notifications when the browser asks. Do the same on your phone; on an iPhone, add the panel to the Home Screen first.")}</p>
          <div>
            <PushHere label={t(keys.push.allow, "Allow here")} variant="primary" onDone={onDone} />
          </div>
        </div>
      )}
    </>
  );
}

function Discord({ onDone }: { onDone: () => void }) {
  const t = useT();
  const id = useId();
  const [ping, setPing] = useState(true);
  return (
    <>
      <Head title="Discord" reads="no_names" />
      <Address
        label={t(keys.add.discord.url, "The channel's webhook")}
        type="url"
        placeholder="https://discord.com/api/webhooks/…"
        hint={t(keys.add.discord.hint, "In Discord: Edit channel, Integrations, Webhooks, Copy URL. Add sends a test.")}
        submit={t(keys.add, "Add")}
        send={(url) => ok(api.POST("/alerts/discord", { body: { url, ping } })).then(onDone)}
      >
        <label htmlFor={id} className="flex items-center gap-2 text-sm text-ink">
          <Checkbox id={id} checked={ping} onCheckedChange={setPing} />
          {t(keys.add.discord.ping, "Urgent ones say @here")}
        </label>
      </Address>
    </>
  );
}

function Slack({ onDone }: { onDone: () => void }) {
  const t = useT();
  return (
    <>
      <Head title="Slack" reads="no_names" />
      <Address
        label={t(keys.add.slack.url, "The channel's incoming webhook")}
        type="url"
        placeholder="https://hooks.slack.com/services/…"
        hint={t(keys.add.slack.hint, "In Slack: your app's Incoming Webhooks, Add to a channel, Copy. Add sends a test.")}
        submit={t(keys.add, "Add")}
        send={(url) => ok(api.POST("/alerts/slack", { body: { url } })).then(onDone)}
      />
    </>
  );
}

function Webhook({ onDone }: { onDone: () => void }) {
  const t = useT();
  const size = useButtonSize();
  const [secret, setSecret] = useState<string | null>(null);
  return (
    <>
      <Head title="Webhook" reads="no_names" />
      {secret === null ? (
        <Address
          label={t(keys.add.webhook.url, "Address")}
          type="url"
          placeholder="https://…"
          hint={t(keys.add.webhook.hint, "JSON, HMAC-signed. The secret is shown once, after Add sends a test.")}
          submit={t(keys.add, "Add")}
          send={(url) => ok(api.POST("/alerts/webhooks", { body: { url } })).then((w) => setSecret(w.secret))}
        />
      ) : (
        <div className="grid gap-3">
          <p className="text-sm text-ink-soft">{t(keys.add.webhook.secret, "What every delivery is signed with (x-signature: t=<unix>,v1=<hex HMAC-SHA256 of t.body>). It is not shown again.")}</p>
          <code className="rounded-md border border-border bg-secondary px-2 py-1.5 font-mono text-xs break-all text-ink">{secret}</code>
          <div className="flex gap-2">
            <Button type="button" size={size("sm")} onClick={() => void navigator.clipboard.writeText(secret).then(() => toast.positive(t(keys.copied, "Copied")))}>
              <Copy aria-hidden className="size-4" />
              {t(keys.copy, "Copy")}
            </Button>
            <Button type="button" variant="outline" size={size("sm")} onClick={onDone}>
              {t(keys.done, "Done")}
            </Button>
          </div>
        </div>
      )}
    </>
  );
}
