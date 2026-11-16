"use client";

import { Badge, Button, Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, Input, toast } from "@evinvest/uikit";
import { ClipboardPaste, ExternalLink } from "lucide-react";
import { useState } from "react";

import { api, ok } from "@/shared/api";
import type { Claude } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { claude as keys } from "../model/i18n";
import { useClaude } from "../model/use-claude";

/**
 * Connects the person's own Claude in three clicks: Connect opens Claude's authorize page, its Copy
 * puts the code on the clipboard, Paste here sends it. Connected, it says so and can disconnect.
 */
export function ConnectClaudeDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const t = useT();
  const mine = useClaude();
  const [connecting, setConnecting] = useState(false);
  const connected = mine.status === "ok" ? mine.data : null;
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) setConnecting(false);
        onOpenChange(o);
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t(keys.title, "Your Claude")}</DialogTitle>
          <DialogDescription>
            {t(keys.why, "The assistant runs on the panel's Claude and costs tokens. Connect your own Claude subscription and its answers cost nothing here.")}
          </DialogDescription>
        </DialogHeader>
        {connected !== null && !connecting ? (
          <ClaudeStatus claude={connected} onReconnect={() => setConnecting(true)} onChanged={mine.reload} />
        ) : (
          <Connect
            onDone={() => {
              setConnecting(false);
              mine.reload();
            }}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function Connect({ onDone }: { onDone: () => void }) {
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
      toast.positive(t(keys.connected, "Your Claude is connected: your answers run on it"));
      onDone();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  if (url === null)
    return (
      <Button size={button()} disabled={busy} onClick={() => void start()}>
        {t(keys.connect, "Connect")}
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

/** A connected Claude: since when, until when, whether Claude still takes it. */
function ClaudeStatus({ claude, onReconnect, onChanged }: { claude: Claude; onReconnect: () => void; onChanged: () => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const [busy, setBusy] = useState(false);
  const failing = claude.checked.error !== null;
  const disconnect = async () => {
    setBusy(true);
    try {
      await ok(api.DELETE("/me/claude"));
      onChanged();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-col gap-3">
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-body-s">
        <dt className="text-ink-soft">{t(keys.state, "State")}</dt>
        <dd className="text-right">
          <Badge tone={failing ? "error" : "positive"}>{failing ? t(keys.failing, "Refused") : t(keys.works, "Connected")}</Badge>
        </dd>
        <dt className="text-ink-soft">{t(keys.since, "Since")}</dt>
        <dd className="text-right text-ink-soft">{formatDateTime(claude.set_at, locale)}</dd>
        <dt className="text-ink-soft">{t(keys.expires, "Expires")}</dt>
        <dd className="text-right text-ink-soft">{formatDateTime(claude.expires_at, locale)}</dd>
      </dl>
      {failing && <p className="text-body-s text-accent-error">{claude.checked.error}</p>}
      <div className="flex flex-wrap gap-2">
        <Button variant={failing ? "primary" : "outline"} size={button("sm")} disabled={busy} onClick={onReconnect}>
          {t(keys.reconnect, "Reconnect")}
        </Button>
        <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => void disconnect()}>
          {t(keys.disconnect, "Disconnect")}
        </Button>
      </div>
    </div>
  );
}
