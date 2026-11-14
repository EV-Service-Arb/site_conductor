"use client";

import {
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  Field,
  FieldDescription,
  FieldLabel,
  Input,
  PageFrame,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Skeleton,
  Switch,
} from "@evinvest/uikit";
import { useId, useState } from "react";

import {
  type Alerts,
  type Destination,
  RULES,
  addChat,
  linkTelegram,
  removeDestination,
  setGmail,
  setRules,
  testDestination,
  useAlerts,
  alerts as alertsKeys,
  type Rule,
} from "@/entities/alerts";
import { type Gmail, fetchGmails } from "@/entities/gmb";
import { type T, useT } from "@/shared/i18n";
import { useResource } from "@/shared/lib/use-resource";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

/** Where the caller's alerts go — their own Telegram, channels, groups and topics they run — and which go where. */
const KIND_NAME = {
  private: (t) => t(alertsKeys.kind.private, "Your chat"),
  channel: (t) => t(alertsKeys.kind.channel, "channel"),
  group: (t) => t(alertsKeys.kind.group, "group"),
} satisfies Record<Destination["kind"], (t: T) => string>;

const RULE_NAME = {
  new_lead: (t) => t(alertsKeys.rule.new_lead, "New leads"),
  contact_overdue: (t) => t(alertsKeys.rule.contact_overdue, "Leads waiting too long"),
  payment_received: (t) => t(alertsKeys.rule.payment_received, "Payments"),
  source_silent: (t) => t(alertsKeys.rule.source_silent, "Silent sources"),
  booked: (t) => t(alertsKeys.rule.booked, "Bookings"),
  access_requested: (t) => t(alertsKeys.rule.access_requested, "Access requests"),
  review_new: (t) => t(alertsKeys.rule.review_new, "New reviews"),
  review_changed: (t) => t(alertsKeys.rule.review_changed, "Edited reviews"),
  review_gone: (t) => t(alertsKeys.rule.review_gone, "Removed reviews"),
  review_reappeared: (t) => t(alertsKeys.rule.review_reappeared, "Reviews back"),
  scan_failed: (t) => t(alertsKeys.rule.scan_failed, "Failed scans"),
} satisfies Record<Rule, (t: T) => string>;

export function AlertsView() {
  const t = useT();
  const alerts = useAlerts();
  const gmails = useResource("gmbs", fetchGmails, "gmbs", { focus: true });
  const [busy, setBusy] = useState(false);
  const act = async (write: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await write();
      alerts.reload();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  return (
    <PageFrame breakpoint="md" title={t(alertsKeys.title, "Alerts")} description={t(alertsKeys.description, "Where your alerts go, and which go where.")} width="content">
      {alerts.status === "loading" ? (
        <Skeleton className="h-40 w-full" />
      ) : alerts.status === "error" ? (
        <ErrorState failure={alerts.failure} onRetry={alerts.reload} />
      ) : !alerts.data.enabled ? (
        <p className="text-sm text-ink-soft">{t(alertsKeys.off, "Telegram is not set up on this server.")}</p>
      ) : (
        <Page alerts={alerts.data} gmails={gmails.status === "ok" ? gmails.data.gmails : []} busy={busy} act={act} />
      )}
    </PageFrame>
  );
}

type Act = (write: () => Promise<unknown>) => Promise<void>;

function Page({ alerts, gmails, busy, act }: { alerts: Alerts; gmails: Gmail[]; busy: boolean; act: Act }) {
  const t = useT();
  const own = alerts.destinations.find((d) => d.kind === "private");
  const chats = alerts.destinations.filter((d) => d.kind !== "private");
  // Opened within the click, or a popup blocker takes it: the link only exists after the POST.
  const connect = () => {
    const tab = window.open("about:blank", "_blank");
    void act(async () => {
      try {
        const url = await linkTelegram();
        if (tab === null) window.location.assign(url);
        else tab.location.href = url;
      } catch (e) {
        tab?.close();
        throw e;
      }
    });
  };

  return (
    <>
      <Section title={t(alertsKeys.own, "Your Telegram")}>
        {own === undefined ? (
          <div className="space-y-1">
            <Button type="button" size="sm" disabled={busy} onClick={connect}>
              {t(alertsKeys.connect, "Connect Telegram")}
            </Button>
            <p className="text-xs text-ink-soft">{t(alertsKeys.connect.hint, "Opens the bot: press Start there.")}</p>
          </div>
        ) : (
          <DestinationCard destination={own} alerts={alerts} gmails={gmails} busy={busy} act={act} onReconnect={connect} />
        )}
      </Section>
      <Section title={t(alertsKeys.chats, "Channels and groups")}>
        {chats.map((d) => (
          <DestinationCard key={d.id} destination={d} alerts={alerts} gmails={gmails} busy={busy} act={act} />
        ))}
        {own === undefined ? (
          <p className="text-sm text-ink-soft">{t(alertsKeys.chats.linkFirst, "Connect your own Telegram first: the bot checks with it that you run the channel.")}</p>
        ) : (
          <AddChat bot={alerts.bot} gmails={gmails} busy={busy} act={act} />
        )}
      </Section>
    </>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Card className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-xs font-medium uppercase tracking-wide text-ink-soft">{title}</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-4 px-4">{children}</CardContent>
    </Card>
  );
}

function DestinationCard({
  destination: d,
  alerts,
  gmails,
  busy,
  act,
  onReconnect,
}: {
  destination: Destination;
  alerts: Alerts;
  gmails: Gmail[];
  busy: boolean;
  act: Act;
  onReconnect?: () => void;
}) {
  const t = useT();
  const name = d.title ?? KIND_NAME[d.kind](t);
  return (
    <div className="space-y-2 rounded-md border border-border p-3">
      <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
        <span className="text-ink">
          {name}
          {d.topic_id !== null && <span className="text-ink-soft"> · {t(alertsKeys.topic, "topic {topic}", { topic: d.topic_id })}</span>}
          {d.kind !== "private" && <span className="text-ink-soft"> · {KIND_NAME[d.kind](t)}</span>}
        </span>
        <span className="flex gap-1">
          <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void act(() => testDestination(d.id))}>
            {t(alertsKeys.test, "Test")}
          </Button>
          <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void act(() => removeDestination(d.id))}>
            {t(alertsKeys.remove, "Remove")}
          </Button>
        </span>
      </div>
      {d.dead !== null && (
        <div className="space-y-1">
          <p className="text-sm text-accent-warn">{t(alertsKeys.dead, "Nothing reaches it: {why}. Fix it, then Test.", { why: d.dead })}</p>
          {onReconnect !== undefined && (
            <Button type="button" size="sm" disabled={busy} onClick={onReconnect}>
              {t(alertsKeys.connect, "Connect Telegram")}
            </Button>
          )}
        </div>
      )}
      {d.kind !== "private" && <p className="text-xs text-ink-soft">{t(alertsKeys.chats.noPii, "Without contact details or buttons: whoever is in the chat reads it.")}</p>}
      {gmails.length > 0 && (
        <GmailScope value={d.gmail_id} gmails={gmails} busy={busy} onChange={(gmail) => void act(() => setGmail(d.id, gmail))} />
      )}
      <Rules alerts={alerts} destination={d} busy={busy} onToggle={(rule, on) => void act(() => setRules(d.id, { [rule]: on }))} />
    </div>
  );
}

const ALL = "all";

function GmailScope({ value, gmails, busy, onChange }: { value: number | null; gmails: Gmail[]; busy: boolean; onChange: (gmail: number | null) => void }) {
  const t = useT();
  const id = useId();
  const size = useControlSize();
  return (
    <Field className="flex flex-col gap-1">
      <FieldLabel htmlFor={id}>{t(alertsKeys.gmail, "Reviews of")}</FieldLabel>
      <Select value={value === null ? ALL : String(value)} onValueChange={(v) => onChange(v === ALL ? null : Number(v))}>
        <SelectTrigger id={id} size={size} className="w-full" disabled={busy}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>{t(alertsKeys.gmail.all, "Every gmail")}</SelectItem>
          {gmails.map((g) => (
            <SelectItem key={g.id} value={String(g.id)}>
              {g.gmail}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  );
}

function Rules({ alerts, destination, busy, onToggle }: { alerts: Alerts; destination: Destination; busy: boolean; onToggle: (rule: (typeof RULES)[number], on: boolean) => void }) {
  const t = useT();
  if (alerts.rules.length === 0) return <p className="text-sm text-ink-soft">{t(alertsKeys.none, "Your access has nothing to alert you of.")}</p>;
  return (
    <div className="space-y-2 border-t border-border pt-2">
      {RULES.filter((rule) => alerts.rules.includes(rule)).map((rule) => (
        <label key={rule} className="flex items-center justify-between gap-2 text-sm text-ink">
          {RULE_NAME[rule](t)}
          <Switch checked={destination.rules[rule] === true} disabled={busy} onCheckedChange={(next) => onToggle(rule, next)} />
        </label>
      ))}
    </div>
  );
}

function AddChat({ bot, gmails, busy, act }: { bot: string | null; gmails: Gmail[]; busy: boolean; act: Act }) {
  const t = useT();
  const id = useId();
  const size = useControlSize();
  const button = useButtonSize();
  const [chat, setChat] = useState("");
  const [gmail, setGmailScope] = useState<number | null>(null);
  const ready = chat.trim() !== "";
  return (
    <form
      className="grid gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) void act(async () => {
          await addChat(chat.trim(), gmail);
          setChat("");
        });
      }}
    >
      <Field className="flex flex-col gap-1">
        <FieldLabel htmlFor={id}>{t(alertsKeys.chats.add, "Channel, group or topic")}</FieldLabel>
        <Input id={id} size={size} autoCapitalize="none" value={chat} onChange={(e) => setChat(e.target.value)} placeholder="@channel" />
        <FieldDescription>{t(alertsKeys.chats.hint, "Add {bot} to it as an admin, then paste @channel, its -100… id, or <group id>/<topic id>.", { bot: bot ?? "…" })}</FieldDescription>
      </Field>
      {gmails.length > 0 && <GmailScope value={gmail} gmails={gmails} busy={busy} onChange={setGmailScope} />}
      <Button type="submit" disabled={busy || !ready} size={button()}>
        {t(alertsKeys.chats.addButton, "Add")}
      </Button>
    </form>
  );
}
