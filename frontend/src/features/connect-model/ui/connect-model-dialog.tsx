"use client";

import { Badge, Button, Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, Input, toast } from "@evinvest/uikit";
import { ClipboardPaste, ExternalLink } from "lucide-react";
import { type ReactNode, useState } from "react";

import { api, ok } from "@/shared/api";
import type { Checked } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { own as keys } from "../model/i18n";
import { useOwnModels } from "../model/use-own-models";

/** The person's own Claude and OpenAI key: each connected, seen to work, disconnected. An offer: without either, answers run on the panel's. */
export function ConnectModelDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const t = useT();
  const mine = useOwnModels();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t(keys.title, "Your own models")}</DialogTitle>
          <DialogDescription>
            {t(keys.why, "On the panel's models the assistant and mail cost tokens. Connect your own and they cost nothing here, and run on yours alone.")}
          </DialogDescription>
        </DialogHeader>
        {mine !== null && (
          <>
            <Own
              title={t(keys.claude, "Claude subscription")}
              state={mine.claude}
              expires={mine.claude?.expires_at ?? null}
              last4={null}
              connect={(done) => <ConnectClaude onDone={done} />}
              disconnect={() => ok(api.DELETE("/me/claude"))}
              onChanged={mine.reload}
            />
            <Own
              title={t(keys.openai, "OpenAI key")}
              state={mine.openai}
              expires={null}
              last4={mine.openai?.last4 ?? null}
              connect={(done) => <ConnectOpenAi onDone={done} />}
              disconnect={() => ok(api.DELETE("/me/openai"))}
              onChanged={mine.reload}
            />
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

function Own({
  title,
  state,
  expires,
  last4,
  connect,
  disconnect,
  onChanged,
}: {
  title: string;
  state: { set_at: string; checked: Checked } | null;
  expires: string | null;
  last4: string | null;
  connect: (onDone: () => void) => ReactNode;
  disconnect: () => Promise<unknown>;
  onChanged: () => void;
}) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const [connecting, setConnecting] = useState(false);
  const [busy, setBusy] = useState(false);
  const failing = state !== null && state.checked.error !== null;
  const drop = async () => {
    setBusy(true);
    try {
      await disconnect();
      onChanged();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="flex flex-col gap-3 border-t border-border-subtle pt-4" aria-label={title}>
      <h3 className="flex items-center gap-2 text-body-s font-medium text-ink">
        <span className="flex-1">
          {title}
          {last4 !== null && <span className="ml-1.5 font-mono text-ink-soft">…{last4}</span>}
        </span>
        {state !== null && <Badge tone={failing ? "error" : "positive"}>{failing ? t(keys.failing, "Refused") : t(keys.works, "Connected")}</Badge>}
      </h3>
      {state === null || connecting ? (
        connect(() => {
          setConnecting(false);
          onChanged();
        })
      ) : (
        <>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-body-s text-ink-soft">
            <dt>{t(keys.since, "Since")}</dt>
            <dd className="text-right">{formatDateTime(state.set_at, locale)}</dd>
            {expires !== null && (
              <>
                <dt>{t(keys.expires, "Expires")}</dt>
                <dd className="text-right">{formatDateTime(expires, locale)}</dd>
              </>
            )}
          </dl>
          {failing && <p className="text-body-s text-accent-error">{state.checked.error}</p>}
          <div className="flex flex-wrap gap-2">
            <Button variant={failing ? "primary" : "outline"} size={button("sm")} disabled={busy} onClick={() => setConnecting(true)}>
              {t(keys.reconnect, "Reconnect")}
            </Button>
            <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => void drop()}>
              {t(keys.disconnect, "Disconnect")}
            </Button>
          </div>
        </>
      )}
    </section>
  );
}

/** In three clicks: Connect opens Claude's authorize page, its Copy puts the code on the clipboard, Paste here sends it. */
function ConnectClaude({ onDone }: { onDone: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const [url, setUrl] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const start = async () => {
    // opened in the click itself: a window opened after an await is a blocked popup
    const popup = window.open("about:blank", "claude-connect", "popup,width=520,height=760");
    setBusy(true);
    try {
      const { url } = await ok(api.POST("/me/claude/connect"));
      setUrl(url);
      if (popup !== null) popup.location.href = url;
    } catch (e) {
      popup?.close();
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  const send = async (pasted: string) => {
    if (pasted.trim() === "") return;
    setBusy(true);
    try {
      await ok(api.PUT("/me/claude", { body: { code: pasted.trim() } }));
      toast.positive(t(keys.connected.claude, "Your Claude is connected: your answers run on it"));
      onDone();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  if (url === null)
    return (
      <Button className="self-start" size={button()} disabled={busy} onClick={() => void start()}>
        {t(keys.connect.claude, "Connect Claude")}
      </Button>
    );
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        void send(code);
      }}
    >
      <p className="text-body-s text-ink-mid">
        {t(keys.steps, "Authorize on Claude's page, press Copy there, then Paste here.")}{" "}
        <a href={url} target="claude-connect" className="inline-flex items-center gap-1 text-ink underline">
          {t(keys.open, "Open Claude's page")}
          <ExternalLink aria-hidden className="size-3.5" />
        </a>
      </p>
      <div className="flex gap-2">
        <Input value={code} onChange={(e) => setCode(e.target.value)} autoComplete="off" aria-label={t(keys.code, "The code Claude shows")} placeholder="…#…" />
        <Button
          type="button"
          size={button()}
          disabled={busy}
          onClick={() =>
            void navigator.clipboard.readText().then(
              (pasted) => {
                setCode(pasted);
                return send(pasted);
              },
              (e: unknown) => notifyFailure(e, t),
            )
          }
        >
          <ClipboardPaste aria-hidden />
          {t(keys.paste, "Paste")}
        </Button>
      </div>
      {code.trim() !== "" && (
        <Button type="submit" variant="outline" size={button("sm")} disabled={busy}>
          {t(keys.send, "Connect with this code")}
        </Button>
      )}
    </form>
  );
}

function ConnectOpenAi({ onDone }: { onDone: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const send = async () => {
    setBusy(true);
    try {
      await ok(api.PUT("/me/openai", { body: { key: key.trim() } }));
      toast.positive(t(keys.connected.openai, "Your OpenAI key is connected: your answers run on it"));
      onDone();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      className="flex gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        void send();
      }}
    >
      <Input type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" aria-label={t(keys.openai, "OpenAI key")} placeholder="sk-…" />
      <Button type="submit" size={button()} disabled={busy || key.trim() === ""}>
        {t(keys.connect, "Connect")}
      </Button>
    </form>
  );
}
