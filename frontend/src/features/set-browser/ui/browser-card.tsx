"use client";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Badge,
  Button,
  Card,
  CardContent,
  Field,
  FieldLabel,
  Input,
  NativeSelect,
  NativeSelectOption,
  Progress,
  buttonVariants,
} from "@evinvest/uikit";
import { Camera, Fingerprint } from "lucide-react";
import { useId, useRef, useState } from "react";

import { type Browser, DEVICES, type Device, PROVIDERS, type Provider, browsersUnconnected, disconnect, removeFilm, setBrowser, testConnection, uploadFilm } from "@/entities/gmb";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

/** A gmail's antidetect browser: one fingerprint and one session for every GBP it manages, out through its proxy. */
export function BrowserCard({ gmail, browser, onChanged }: { gmail: number; browser: Browser | null; onChanged: () => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const [editing, setEditing] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [busy, setBusy] = useState(false);
  const [unconnected, setUnconnected] = useState(false);
  const act = async (run: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await run();
      onChanged();
      setEditing(false);
    } catch (e) {
      if (browsersUnconnected(e)) setUnconnected(true);
      else notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  const failing = browser !== null && browser.checked.error !== null;

  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-3 px-4">
        <header className="flex items-center gap-2">
          <Fingerprint aria-hidden className="size-4 text-ink-soft" />
          <h3 className="flex-1 text-sm font-semibold text-ink">{t("browser.title")}</h3>
          {browser && <Badge tone={failing ? "error" : "positive"}>{t(failing ? "browser.failing" : "browser.healthy")}</Badge>}
        </header>
        {browser && !editing ? (
          <>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-ink-soft">{t("browser.profile")}</dt>
              <dd className="text-right text-ink">
                {t(`browser.provider.${browser.provider}`)} · {browser.provider === "multilogin" ? browser.device : t(`browser.device.${browser.device as Device}`)}
              </dd>
              <dt className="text-ink-soft">{t("browser.proxy")}</dt>
              <dd className="truncate text-right font-mono text-ink">{browser.endpoint}</dd>
              <dt className="text-ink-soft">{t("browser.exit")}</dt>
              <dd className="truncate text-right font-mono text-ink">
                {[browser.exit.country, browser.exit.city, browser.exit.mobile ? t("browser.mobile") : t("browser.fixed"), browser.exit.ip].join(" · ")}
              </dd>
              <dt className="text-ink-soft">{t("browser.set")}</dt>
              <dd className="text-right text-ink-soft">{formatDateTime(browser.set_at, locale)}</dd>
            </dl>
            <p className="text-xs text-ink-soft">
              {t("account.checkedAt", { at: formatDateTime(browser.checked.at, locale) })}
              {browser.checked.error !== null && <span className="block text-accent-error">{browser.checked.error}</span>}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" size={button("sm")} disabled={busy} onClick={() => void act(() => testConnection(gmail, "browser"))}>
                {t("account.test")}
              </Button>
              <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => setEditing(true)}>
                {t("browser.replaceProxy")}
              </Button>
              <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => setRemoving(true)}>
                {t("account.remove")}
              </Button>
            </div>
            {browser.provider !== "multilogin_browser" && <Film gmail={gmail} browser={browser} onChanged={onChanged} />}
          </>
        ) : (
          <BrowserForm
            set={browser}
            busy={busy}
            onCancel={browser ? () => setEditing(false) : undefined}
            onSubmit={(v) => void act(() => setBrowser(gmail, v))}
          />
        )}
        {unconnected && <p className="text-xs text-accent-warn">{t("browser.unconnected")}</p>}
        <p className="text-xs text-ink-soft">{t("browser.why")}</p>
      </CardContent>
      <AlertDialog open={removing} onOpenChange={setRemoving}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("browser.remove.title")}</AlertDialogTitle>
            <AlertDialogDescription>{t("browser.remove.body")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel className={buttonVariants({ variant: "outline", size: button() })}>{t("account.cancel")}</AlertDialogCancel>
            <AlertDialogAction className={buttonVariants({ variant: "destructive", size: button() })} onClick={() => void act(() => disconnect(gmail, "browser"))}>
              {t("account.remove")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}

/** The server takes up to this much (a phone's: Multilogin's Library's): a bigger file is refused before it is sent. */
const FILM_MAX = 512 * 1024 * 1024;
const PHONE_FILM_MAX = 100 * 1024 * 1024;

/**
 * The board's "plug to submit a selected vid": a film the browser's next launch plays as its back camera; a cloud
 * phone's goes to Multilogin's Library, picked in its Camera panel.
 */
function Film({ gmail, browser, onChanged }: { gmail: number; browser: Browser; onChanged: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const picker = useRef<HTMLInputElement>(null);
  const [sent, setSent] = useState<number | null>(null);
  const phone = browser.provider === "multilogin";
  const running = !phone && browser.running !== null;
  const act = async (run: () => Promise<unknown>) => {
    setSent(0);
    try {
      await run();
      onChanged();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setSent(null);
    }
  };
  const upload = (file: File) => {
    if (file.size > (phone ? PHONE_FILM_MAX : FILM_MAX)) return void notifyFailure(new Error(t(phone ? "browser.film.phone.tooBig" : "browser.film.tooBig")), t);
    void act(() => uploadFilm(gmail, file, setSent));
  };
  return (
    <section className="flex flex-col gap-2 border-t border-border pt-3">
      <h4 className="flex items-center gap-2 text-sm font-medium text-ink">
        <Camera aria-hidden className="size-4 text-ink-soft" />
        {t("browser.film.title")}
      </h4>
      <p className="text-xs text-ink-soft">{t(phone ? (browser.film ? "browser.film.phone.set" : "browser.film.phone.none") : browser.film ? "browser.film.set" : "browser.film.none")}</p>
      {sent !== null ? (
        <div className="flex items-center gap-2 text-xs text-ink-soft">
          <Progress value={Math.round(sent * 100)} aria-label={t("browser.film.uploading")} className="flex-1" />
          <span className="font-mono tabular-nums">{Math.round(sent * 100)}%</span>
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size={button("sm")} disabled={running} onClick={() => picker.current!.click()}>
            {t(browser.film ? "account.replace" : "browser.film.upload")}
          </Button>
          {browser.film && (
            <Button variant="ghost" size={button("sm")} disabled={running} onClick={() => void act(() => removeFilm(gmail))}>
              {t("account.remove")}
            </Button>
          )}
        </div>
      )}
      {running && <p className="text-xs text-accent-warn">{t("browser.film.running")}</p>}
      <input
        ref={picker}
        type="file"
        accept="video/mp4"
        hidden
        onChange={(e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (file) upload(file);
        }}
      />
    </section>
  );
}

/** A new browser, or the proxy replaced under the one `set`: its provider and device are fixed once chosen. */
function BrowserForm({
  set,
  busy,
  onCancel,
  onSubmit,
}: {
  set: Browser | null;
  busy: boolean;
  onCancel: (() => void) | undefined;
  onSubmit: (v: { provider: Provider; device: Device | null; proxy: string }) => void;
}) {
  const t = useT();
  const id = useId();
  const button = useButtonSize();
  const size = useControlSize();
  const [provider, setProvider] = useState<Provider | "">(set?.provider ?? "");
  const [device, setDevice] = useState<Device | "">(set === null || set.provider === "multilogin" ? "" : (set.device as Device));
  const [proxy, setProxy] = useState("");
  // a cloud phone's model is drawn by the server from Multilogin's
  const phone = provider === "multilogin";
  const ready = provider !== "" && (phone || device !== "") && proxy.trim() !== "";
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) onSubmit({ provider, device: phone || device === "" ? null : device, proxy: proxy.trim() });
      }}
    >
      <div className="grid gap-3 sm:grid-cols-2">
        <Field>
          <FieldLabel htmlFor={`${id}-provider`}>{t("browser.provider")}</FieldLabel>
          <NativeSelect id={`${id}-provider`} size={size} disabled={set !== null} value={provider} onChange={(e) => setProvider(e.target.value as Provider)}>
            {provider === "" && <NativeSelectOption value="">—</NativeSelectOption>}
            {PROVIDERS.map((p) => (
              <NativeSelectOption key={p} value={p}>
                {t(`browser.provider.${p}`)}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </Field>
        {phone ? (
          <p className="self-end text-xs text-ink-soft">{set === null ? t("browser.device.drawn") : set.device}</p>
        ) : (
          <Field>
            <FieldLabel htmlFor={`${id}-device`}>{t("browser.device")}</FieldLabel>
            <NativeSelect id={`${id}-device`} size={size} disabled={set !== null} value={device} onChange={(e) => setDevice(e.target.value as Device)}>
              {device === "" && <NativeSelectOption value="">—</NativeSelectOption>}
              {DEVICES.map((d) => (
                <NativeSelectOption key={d} value={d}>
                  {t(`browser.device.${d}`)}
                </NativeSelectOption>
              ))}
            </NativeSelect>
          </Field>
        )}
      </div>
      <Field>
        <FieldLabel htmlFor={`${id}-proxy`}>{t("browser.proxy")}</FieldLabel>
        <Input
          id={`${id}-proxy`}
          size={size}
          type="password"
          value={proxy}
          onChange={(e) => setProxy(e.target.value)}
          autoComplete="off"
          spellCheck={false}
          placeholder="socks5://user:pass@host:port"
        />
      </Field>
      <p className="text-xs text-ink-soft">{t(set === null ? "browser.how" : "browser.how.replace")}</p>
      <div className="flex gap-2">
        <Button type="submit" size={button("sm")} disabled={!ready || busy}>
          {t(set === null ? "browser.set.submit" : "browser.replaceProxy")}
        </Button>
        {onCancel && (
          <Button type="button" variant="ghost" size={button("sm")} disabled={busy} onClick={onCancel}>
            {t("account.cancel")}
          </Button>
        )}
      </div>
    </form>
  );
}
