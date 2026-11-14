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
import { type T, useLocale, useT } from "@/shared/i18n";
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
          <h3 className="flex-1 text-sm font-semibold text-ink">{t("Antidetect environment")}</h3>
          {browser && <Badge tone={failing ? "error" : "positive"}>{(failing ? t("Failing") : t("Healthy"))}</Badge>}
        </header>
        {browser && !editing ? (
          <>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-ink-soft">{t("Profile")}</dt>
              <dd className="text-right text-ink">
                {PROVIDER[browser.provider](t)} · {browser.provider === "multilogin" ? browser.device : DEVICE[browser.device as Device](t)}
              </dd>
              <dt className="text-ink-soft">{t("Proxy")}</dt>
              <dd className="truncate text-right font-mono text-ink">{browser.endpoint}</dd>
              <dt className="text-ink-soft">{t("Exit")}</dt>
              <dd className="truncate text-right font-mono text-ink">
                {[browser.exit.country, browser.exit.city, browser.exit.mobile ? t("mobile") : t("fixed line"), browser.exit.ip].join(" · ")}
              </dd>
              <dt className="text-ink-soft">{t("Set")}</dt>
              <dd className="text-right text-ink-soft">{formatDateTime(browser.set_at, locale)}</dd>
            </dl>
            <p className="text-xs text-ink-soft">
              {t("Checked {at}", { at: formatDateTime(browser.checked.at, locale) })}
              {browser.checked.error !== null && <span className="block text-accent-error">{browser.checked.error}</span>}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" size={button("sm")} disabled={busy} onClick={() => void act(() => testConnection(gmail, "browser"))}>
                {t("Test")}
              </Button>
              <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => setEditing(true)}>
                {t("Replace proxy")}
              </Button>
              <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => setRemoving(true)}>
                {t("Remove")}
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
        {unconnected && <p className="text-xs text-accent-warn">{t("Browsers not connected")}</p>}
        <p className="text-xs text-ink-soft">{t("One fingerprint and one session for every GBP of this gmail, out through its proxy in their country.")}</p>
      </CardContent>
      <AlertDialog open={removing} onOpenChange={setRemoving}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("Remove the browser?")}</AlertDialogTitle>
            <AlertDialogDescription>{t("Its profile goes with it: the fingerprint, the cookies, the signed-in session. A new one starts from nothing.")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel className={buttonVariants({ variant: "outline", size: button() })}>{t("Cancel")}</AlertDialogCancel>
            <AlertDialogAction className={buttonVariants({ variant: "destructive", size: button() })} onClick={() => void act(() => disconnect(gmail, "browser"))}>
              {t("Remove")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}

/** The server takes up to this much (a phone's: Multilogin's Library's): a bigger file is refused before it is sent. */
const PROVIDER = {
  gologin: (t) => t("GoLogin"),
  multilogin: (t) => t("Multilogin · cloud phone"),
  multilogin_browser: (t) => t("Multilogin · browser"),
} satisfies Record<Provider, (t: T) => string>;
const DEVICE = { pixel_7: (t) => t("Pixel 7"), galaxy_s23: (t) => t("Galaxy S23"), redmi_note_14: (t) => t("Redmi Note 14") } satisfies Record<Device, (t: T) => string>;

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
    if (file.size > (phone ? PHONE_FILM_MAX : FILM_MAX)) return void notifyFailure(new Error((phone ? t("The film is over 100 MiB: Multilogin's Library takes no more.") : t("The film is over 512 MiB."))), t);
    void act(() => uploadFilm(gmail, file, setSent));
  };
  return (
    <section className="flex flex-col gap-2 border-t border-border pt-3">
      <h4 className="flex items-center gap-2 text-sm font-medium text-ink">
        <Camera aria-hidden className="size-4 text-ink-soft" />
        {t("Film as camera")}
      </h4>
      <p className="text-xs text-ink-soft">{(phone ? (browser.film ? t("Film in Multilogin's Library: pick it in the phone's Camera panel in the Multilogin app.") : t("An mp4 (up to 100 MiB) for Multilogin's Library: pick it in the phone's Camera panel in the Multilogin app.")) : (browser.film ? t("Film set — the next start plays it as the back camera (5 s still, the film, last frame held 60 s).") : t("An mp4 the next start plays as the back camera, its sound as the mic.")))}</p>
      {sent !== null ? (
        <div className="flex items-center gap-2 text-xs text-ink-soft">
          <Progress value={Math.round(sent * 100)} aria-label={t("Uploading the film")} className="flex-1" />
          <span className="font-mono tabular-nums">{Math.round(sent * 100)}%</span>
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size={button("sm")} disabled={running} onClick={() => picker.current!.click()}>
            {(browser.film ? t("Replace") : t("Upload an mp4"))}
          </Button>
          {browser.film && (
            <Button variant="ghost" size={button("sm")} disabled={running} onClick={() => void act(() => removeFilm(gmail))}>
              {t("Remove")}
            </Button>
          )}
        </div>
      )}
      {running && <p className="text-xs text-accent-warn">{t("The browser runs: stop it first, the next launch plays the film.")}</p>}
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
          <FieldLabel htmlFor={`${id}-provider`}>{t("Provider")}</FieldLabel>
          <NativeSelect id={`${id}-provider`} size={size} disabled={set !== null} value={provider} onChange={(e) => setProvider(e.target.value as Provider)}>
            {provider === "" && <NativeSelectOption value="">—</NativeSelectOption>}
            {PROVIDERS.map((p) => (
              <NativeSelectOption key={p} value={p}>
                {PROVIDER[p](t)}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </Field>
        {phone ? (
          <p className="self-end text-xs text-ink-soft">{set === null ? t("A real Android phone; its model is drawn from Multilogin's Google and Samsung ones.") : set.device}</p>
        ) : (
          <Field>
            <FieldLabel htmlFor={`${id}-device`}>{t("Device")}</FieldLabel>
            <NativeSelect id={`${id}-device`} size={size} disabled={set !== null} value={device} onChange={(e) => setDevice(e.target.value as Device)}>
              {device === "" && <NativeSelectOption value="">—</NativeSelectOption>}
              {DEVICES.map((d) => (
                <NativeSelectOption key={d} value={d}>
                  {DEVICE[d](t)}
                </NativeSelectOption>
              ))}
            </NativeSelect>
          </Field>
        )}
      </div>
      <Field>
        <FieldLabel htmlFor={`${id}-proxy`}>{t("Proxy")}</FieldLabel>
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
      <p className="text-xs text-ink-soft">{(set === null ? t("socks5:// or http://, user:pass@host:port. Kept only once it exits in the country of this gmail's GBPs; the provider and device are set once.") : t("The same profile, out through another proxy; checked before it is kept."))}</p>
      <div className="flex gap-2">
        <Button type="submit" size={button("sm")} disabled={!ready || busy}>
          {(set === null ? t("Check and set up") : t("Replace proxy"))}
        </Button>
        {onCancel && (
          <Button type="button" variant="ghost" size={button("sm")} disabled={busy} onClick={onCancel}>
            {t("Cancel")}
          </Button>
        )}
      </div>
    </form>
  );
}
