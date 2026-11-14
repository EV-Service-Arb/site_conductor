"use client";

import { Button, Input, Settled, Skeleton, StatusDot, buttonVariants, cn, toast } from "@evinvest/uikit";
import { ArrowLeft, ArrowRight, Hand, Plus, X } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";

import { type Browser, type BrowserIn, type BrowserOut, type Gmail, browsersUnconnected, fetchConnections, openAccount, startBrowser, stopBrowser, useGmails } from "@/entities/gmb";
import { ApiError } from "@/shared/api";
import { type T, useT } from "@/shared/i18n";
import { backoffDelay } from "@/shared/lib/live";
import { useResource } from "@/shared/lib/use-resource";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { CLOSE, KEYS, parseOut, textFrames } from "../model/protocol";

/** Types into the field focused in the browser; `null` while no socket is open. */
export type TypeIn = ((text: string) => void) | null;

/**
 * A gmail's browser, live: its page as frames, the operator's touches and keys sent back. `gmb`: started at that GBP's point, `null`
 * with the position blocked. `aside` sits beside the page.
 */
export function BrowserSession({ gmail, gmb, aside }: { gmail: Gmail; gmb: number | null; aside?: (type: TypeIn) => ReactNode }) {
  const t = useT();
  const button = useButtonSize();
  const connections = useResource(`connections.${gmail.id}`, () => fetchConnections(gmail.id), "connections", { live: "browsers" });
  return (
    <Settled loading={connections.status === "loading"} skeleton={<Skeleton className="h-96 w-full" />}>
      {connections.status === "error" && <ErrorState failure={connections.failure} onRetry={connections.reload} />}
      {connections.status === "ok" &&
        (connections.data.browser === null ? (
          <EmptyState title={t("No browser for this gmail yet")} description={t("Set up {gmail}'s antidetect environment on its Connections tab.", { gmail: gmail.gmail })}>
            <Button size={button("sm")} onClick={() => openAccount({ gmail: gmail.id, tab: "connections" })}>
              {t("Set it up")}
            </Button>
          </EmptyState>
        ) : (
          <Session key={`${gmail.id}:${gmb}`} gmail={gmail} gmb={gmb} browser={connections.data.browser} reload={connections.reload} aside={aside} />
        ))}
    </Settled>
  );
}

/** `elsewhere`: it runs, for another GBP or for the gmail alone. */
type Phase = "off" | "elsewhere" | "starting" | "connecting" | "open" | "taken" | "lost";
const PHASE = {
  off: (t) => t("stopped"),
  elsewhere: (t) => t("running elsewhere"),
  starting: (t) => t("starting"),
  connecting: (t) => t("connecting"),
  open: (t) => t("running"),
  taken: (t) => t("taken over"),
  lost: (t) => t("disconnected"),
} satisfies Record<Phase, (t: T) => string>;
type Runs = "no" | "here" | "elsewhere";
interface Page {
  frame: { src: string; width: number; height: number } | null;
  tabs: { id: string; title: string; url: string }[];
  active: string | null;
  dialog: { kind: string; message: string; default: string | null } | null;
}
const BLANK: Page = { frame: null, tabs: [], active: null, dialog: null };

function Session({ gmail, gmb, browser, reload, aside }: { gmail: Gmail; gmb: number | null; browser: Browser; reload: () => void; aside: ((type: TypeIn) => ReactNode) | undefined }) {
  const t = useT();
  const button = useButtonSize();
  const runs: Runs = browser.running === null ? "no" : browser.running.gmb === gmb ? "here" : "elsewhere";
  const [phase, setPhase] = useState<Phase>(runs === "here" ? "connecting" : runs === "no" ? "off" : "elsewhere");
  const [link, setLink] = useState<{ n: number; delay: number } | null>(runs === "here" ? { n: 0, delay: 0 } : null);
  const [page, setPage] = useState<Page>(BLANK);
  const [failure, setFailure] = useState<string | null>(null);
  const [socket, setSocket] = useState<WebSocket | null>(null);
  const attempts = useRef(0);
  // started or stopped elsewhere (another tab, another viewer): what is shown while no page is follows it
  const [seen, setSeen] = useState(runs);
  if (seen !== runs) {
    setSeen(runs);
    if (phase === "off" || phase === "elsewhere") {
      if (runs === "here") {
        setPhase("connecting");
        setLink((l) => ({ n: (l?.n ?? 0) + 1, delay: 0 }));
      } else setPhase(runs === "no" ? "off" : "elsewhere");
    }
  }

  const { id } = gmail;
  useEffect(() => {
    if (link === null) return;
    let ws: WebSocket | null = null;
    let opened = false;
    const gone = () => {
      setPhase("off");
      setPage(BLANK);
      reload();
    };
    const timer = window.setTimeout(() => {
      const url = new URL(`/api/v1/gmails/${id}/browser`, window.location.href);
      url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
      const opening = new WebSocket(url);
      ws = opening;
      ws.onopen = () => {
        opened = true;
        setSocket(opening);
        attempts.current = 0;
        setPhase("open");
      };
      ws.onmessage = (e) => {
        const out = parseOut(String(e.data));
        if (out.type === "error") toast.error(out.message);
        else setPage((p) => apply(p, out));
      };
      ws.onclose = (e) => {
        setSocket(null);
        if (e.code === CLOSE.signedOut) {
          setPhase("lost");
          window.dispatchEvent(new Event("sa:unauthenticated"));
        } else if (e.code === CLOSE.notRunning || e.code === CLOSE.stopped) gone();
        else if (e.code === CLOSE.takenOver) setPhase("taken");
        else if (opened || e.code === CLOSE.goingAway) {
          setPhase("connecting");
          setLink({ n: link.n + 1, delay: backoffDelay(attempts.current++, Math.random) });
        } else setPhase("lost");
      };
    }, link.delay);
    return () => {
      window.clearTimeout(timer);
      if (ws !== null) {
        ws.onclose = null;
        ws.close();
      }
      setSocket(null);
    };
  }, [link, id, reload]);

  const connect = () => {
    attempts.current = 0;
    setPhase("connecting");
    setLink((l) => ({ n: (l?.n ?? 0) + 1, delay: 0 }));
  };
  const start = async (restart: boolean) => {
    setPhase("starting");
    setFailure(null);
    let stopped = !restart;
    try {
      if (restart) {
        await stopBrowser(gmail.id);
        stopped = true;
      }
      await startBrowser(gmail.id, gmb);
    } catch (e) {
      setPhase(stopped ? "off" : "elsewhere");
      if (!(e instanceof ApiError)) throw e;
      if (browsersUnconnected(e)) setFailure(t("Browsers not connected"));
      // the runner full: its own words
      else if (e.failure.kind === "unavailable" && e.failure.message !== "") setFailure(e.failure.message);
      else if (e.failure.kind !== "unauthenticated") setFailure(failureText(e.failure, t));
      return;
    }
    reload();
    connect();
  };
  const stop = async () => {
    try {
      await stopBrowser(gmail.id);
    } catch (e) {
      notifyFailure(e, t);
      return;
    }
    setLink(null);
    setPhase("off");
    setPage(BLANK);
    reload();
  };
  const send = (frame: BrowserIn) => {
    if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(frame));
  };
  const typeIn: TypeIn = phase === "open" ? (text) => textFrames(text).forEach(send) : null;
  const activeTab = page.tabs.find((tab) => tab.id === page.active) ?? null;

  return (
    <div className="flex flex-col gap-4 xl:flex-row xl:items-start">
      <div className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-lg border border-border">
        <div className="flex items-center gap-2 border-b border-border bg-hover/40 px-3 py-2 text-xs">
          <StatusDot tone={phase === "open" ? "positive" : phase === "connecting" || phase === "starting" ? "warn" : "muted"} />
          <span className="font-medium text-ink">{PHASE[phase](t)}</span>
          <span className="min-w-0 flex-1 truncate font-mono text-ink-soft">
            {[activeTab?.url, browser.exit.country, browser.exit.city, browser.exit.mobile ? t("mobile") : t("fixed line"), browser.exit.ip].filter(Boolean).join(" · ")}
          </span>
          {browser.running?.stream && (
            <a href={browser.running.stream} target="_blank" rel="noopener noreferrer" className={buttonVariants({ variant: "ghost", size: button("sm") })}>
              {t("Open Multilogin's view")}
            </a>
          )}
          {phase === "open" || phase === "connecting" || phase === "taken" || phase === "elsewhere" ? (
            <Button variant="outline" size={button("sm")} onClick={() => void stop()}>
              {t("Stop")}
            </Button>
          ) : null}
        </div>
        {phase === "open" && <Chrome page={page} phone={browser.provider === "multilogin"} send={send} />}
        <div className="relative flex h-[calc(100svh-27rem)] min-h-96 items-center justify-center bg-black/40">
          {phase === "open" && page.frame !== null ? (
            <Screen frame={page.frame} send={send} onPaste={(text) => textFrames(text).forEach(send)} />
          ) : (
            <Idle phase={phase} browser={browser} failure={failure} onStart={(restart) => void start(restart)} onConnect={connect} />
          )}
          {phase === "open" && page.dialog !== null && <Dialog dialog={page.dialog} send={send} />}
        </div>
      </div>
      {aside?.(typeIn)}
    </div>
  );
}

function apply(page: Page, out: Exclude<BrowserOut, { type: "error" }>): Page {
  switch (out.type) {
    case "frame":
      return { ...page, frame: { src: `data:image/jpeg;base64,${out.data}`, width: out.width, height: out.height } };
    case "tabs":
      return { ...page, tabs: out.tabs, active: out.active };
    case "dialog":
      return { ...page, dialog: { kind: out.kind, message: out.message, default: out.default } };
    case "dialog_closed":
      return { ...page, dialog: null };
  }
}

/** What stands in for the page while none is shown: a Start, a reconnect, or why not. */
function Idle({ phase, browser, failure, onStart, onConnect }: { phase: Phase; browser: Browser; failure: string | null; onStart: (restart: boolean) => void; onConnect: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const gmails = useGmails();
  const runsFor = browser.running?.gmb ?? null;
  // a GBP the list read before it was added: its id until the list catches up
  const name = runsFor === null ? null : ((gmails.status === "ok" ? gmails.data.gmbs.find((g) => g.id === runsFor)?.name : undefined) ?? `#${runsFor}`);
  return (
    <div className="flex max-w-sm flex-col items-center gap-3 p-6 text-center text-sm">
      {phase === "elsewhere" ? (
        <>
          <p className="text-ink-soft">{name === null ? t("It runs for the gmail alone, the position blocked.") : t("It runs for {name}, at its point.", { name })}</p>
          <div className="flex flex-wrap justify-center gap-2">
            <Button size={button("sm")} onClick={() => onStart(true)}>
              {t("Stop and start here")}
            </Button>
            <Button variant="outline" size={button("sm")} onClick={onConnect}>
              {t("Watch it as it runs")}
            </Button>
          </div>
        </>
      ) : phase === "off" || phase === "starting" ? (
        <>
          <p className="text-ink-soft">{t("The browser is not running.")}</p>
          <Button size={button("sm")} disabled={phase === "starting"} onClick={() => onStart(false)}>
            {(phase === "starting" ? t("Starting…") : t("Start"))}
          </Button>
          {browser.checked.error !== null && <p className="text-xs text-accent-warn">{t("Its last check failed: {error}", { error: browser.checked.error })}</p>}
        </>
      ) : phase === "taken" ? (
        <>
          <p className="text-ink-soft">{t("Another viewer of this gmail took over.")}</p>
          <Button size={button("sm")} onClick={onConnect}>
            {t("Take back")}
          </Button>
        </>
      ) : phase === "lost" ? (
        <>
          <p className="text-ink-soft">{t("The connection to the browser was lost.")}</p>
          <Button variant="outline" size={button("sm")} onClick={onConnect}>
            {t("Reconnect")}
          </Button>
        </>
      ) : (
        <p className="text-ink-soft">{t("connecting")}</p>
      )}
      {failure !== null && <p className="text-xs text-accent-error">{failure}</p>}
    </div>
  );
}

/** The browser's own tabs and address bar, drawn here: the frames show the page alone. A phone's has neither tabs nor forward: Android's own UI is in its frames. */
function Chrome({ page, phone, send }: { page: Page; phone: boolean; send: (frame: BrowserIn) => void }) {
  const t = useT();
  const button = useButtonSize();
  const active = page.tabs.find((tab) => tab.id === page.active) ?? null;
  return (
    <div className="flex flex-col border-b border-border">
      {!phone && <div role="tablist" aria-label={t("The browser's tabs")} className="flex items-center gap-1 overflow-x-auto px-2 pt-1.5">
        {page.tabs.map((tab) => (
          <span key={tab.id} className={cn("flex max-w-48 items-center gap-1 rounded-t-md border border-b-0 px-2 py-1 text-xs", tab.id === page.active ? "border-border bg-background text-ink" : "border-transparent text-ink-soft hover:text-ink")}>
            <button type="button" role="tab" aria-selected={tab.id === page.active} title={tab.url} className="min-w-0 truncate outline-none focus-visible:underline" onClick={() => send({ type: "activate", id: tab.id })}>
              {tab.title || tab.url}
            </button>
            <button type="button" aria-label={t("Close {title}", { title: tab.title || tab.url })} className="shrink-0 rounded text-ink-soft hover:text-ink" onClick={() => send({ type: "close_tab", id: tab.id })}>
              <X aria-hidden className="size-3" />
            </button>
          </span>
        ))}
        <Button variant="ghost" size={button("sm")} icon aria-label={t("New tab")} title={t("New tab")} onClick={() => send({ type: "new_tab" })}>
          <Plus aria-hidden />
        </Button>
      </div>}
      <div className="flex items-center gap-1 px-2 py-1.5">
        <Button variant="ghost" size={button("sm")} icon aria-label={t("Back")} title={t("Back")} onClick={() => send({ type: "history", delta: -1 })}>
          <ArrowLeft aria-hidden />
        </Button>
        {!phone && (
          <Button variant="ghost" size={button("sm")} icon aria-label={t("Forward")} title={t("Forward")} onClick={() => send({ type: "history", delta: 1 })}>
            <ArrowRight aria-hidden />
          </Button>
        )}
        {/* a page that moves on its own resets what was being typed */}
        <UrlBar key={active?.url ?? ""} url={active?.url ?? ""} onGo={(url) => send({ type: "navigate", url })} />
      </div>
    </div>
  );
}

function UrlBar({ url, onGo }: { url: string; onGo: (url: string) => void }) {
  const t = useT();
  const size = useControlSize();
  const [typed, setTyped] = useState(url);
  return (
    <form
      className="min-w-0 flex-1"
      onSubmit={(e) => {
        e.preventDefault();
        const v = typed.trim();
        if (v !== "") onGo(/^[a-z][a-z0-9+.-]*:/i.test(v) ? v : `https://${v}`);
      }}
    >
      <Input size={size} value={typed} onChange={(e) => setTyped(e.target.value)} aria-label={t("Address")} spellCheck={false} autoComplete="off" className="font-mono text-xs" />
    </form>
  );
}

/** The page: pointer as touch (the browser poses as a phone), wheel, and keys while focused, all in the page's CSS px. */
function Screen({ frame, send, onPaste }: { frame: NonNullable<Page["frame"]>; send: (frame: BrowserIn) => void; onPaste: (text: string) => void }) {
  const t = useT();
  const img = useRef<HTMLImageElement>(null);
  const pressed = useRef(false);
  const [focused, setFocused] = useState(false);
  // where a point on the element falls on the page: the picture is contained, so it may be letterboxed
  const at = (e: { clientX: number; clientY: number }) => {
    const r = img.current!.getBoundingClientRect();
    const { width, height } = frame;
    const scale = Math.min(r.width / width, r.height / height);
    const x = (e.clientX - r.left - (r.width - width * scale) / 2) / scale;
    const y = (e.clientY - r.top - (r.height - height * scale) / 2) / scale;
    return { x: Math.min(width, Math.max(0, x)), y: Math.min(height, Math.max(0, y)) };
  };
  // React's wheel listener is passive: the page under it would scroll too
  useEffect(() => {
    const el = img.current!;
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      send({ type: "wheel", ...at(e), dx: e.deltaX, dy: e.deltaY });
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => el.removeEventListener("wheel", wheel);
  });
  return (
    <div
      tabIndex={0}
      role="application"
      aria-label={t("The browser's page")}
      className="relative flex h-full w-full items-center justify-center outline-none"
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
      onKeyDown={(e) => {
        if (e.metaKey) return;
        if (e.ctrlKey && !e.altKey) {
          if (e.key === "a") {
            e.preventDefault();
            send({ type: "key", key: "a", ctrl: true });
          }
          return;
        }
        if (KEYS.has(e.key)) {
          e.preventDefault();
          send({ type: "key", key: e.key, ctrl: false });
          return;
        }
        // an Alt chord is no text (the shell's leader never gets here); AltGr (Ctrl+Alt) composes characters
        if (Array.from(e.key).length !== 1 || (e.altKey && !e.ctrlKey)) return;
        e.preventDefault();
        send({ type: "text", text: e.key });
      }}
      onPaste={(e) => {
        e.preventDefault();
        onPaste(e.clipboardData.getData("text"));
      }}
    >
      {/* eslint-disable-next-line @next/next/no-img-element -- a data URL per frame: nothing for next/image to optimise */}
      <img
        ref={img}
        src={frame.src}
        alt={t("The browser's page")}
        draggable={false}
        style={{ aspectRatio: `${frame.width} / ${frame.height}` }}
        className="h-full max-w-full touch-none object-contain select-none"
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          (e.currentTarget.parentElement as HTMLElement).focus();
          pressed.current = true;
          send({ type: "touch", phase: "start", ...at(e) });
        }}
        onPointerMove={(e) => pressed.current && send({ type: "touch", phase: "move", ...at(e) })}
        onPointerUp={(e) => {
          pressed.current = false;
          send({ type: "touch", phase: "end", ...at(e) });
        }}
        onPointerCancel={(e) => {
          pressed.current = false;
          send({ type: "touch", phase: "end", ...at(e) });
        }}
      />
      <span className={cn("pointer-events-none absolute bottom-3 left-3 flex items-center gap-1.5 rounded-md border border-border bg-background/90 px-2 py-1 text-xs", focused ? "text-ink" : "text-ink-soft")}>
        <Hand aria-hidden className="size-3.5" />
        {(focused ? t("You have control · keys and clicks go to the page") : t("Click the page to take control"))}
      </span>
    </div>
  );
}

/** The page's alert, confirm or prompt, answered here: the page waits on it. */
function Dialog({ dialog, send }: { dialog: NonNullable<Page["dialog"]>; send: (frame: BrowserIn) => void }) {
  const t = useT();
  const button = useButtonSize();
  const size = useControlSize();
  const [text, setText] = useState(dialog.default ?? "");
  const prompt = dialog.kind === "prompt";
  return (
    <form
      role="alertdialog"
      aria-label={t("The page asks")}
      className="absolute top-1/3 left-1/2 flex w-80 max-w-[calc(100%-2rem)] -translate-x-1/2 flex-col gap-3 rounded-lg border border-border bg-background p-4 shadow-overlay"
      onSubmit={(e) => {
        e.preventDefault();
        send({ type: "dialog", accept: true, text: prompt ? text : null });
      }}
    >
      <p className="text-xs text-ink-soft">{t("The page asks")}</p>
      <p className="text-sm whitespace-pre-wrap text-ink">{dialog.message}</p>
      {prompt && <Input size={size} value={text} onChange={(e) => setText(e.target.value)} autoFocus />}
      <div className="flex justify-end gap-2">
        {dialog.kind !== "alert" && (
          <Button type="button" variant="ghost" size={button("sm")} onClick={() => send({ type: "dialog", accept: false, text: null })}>
            {t("Cancel")}
          </Button>
        )}
        <Button type="submit" size={button("sm")}>
          {t("OK")}
        </Button>
      </div>
    </form>
  );
}
