/**
 * A gmail's antidetect browser for the dev stub: `/api/v1/gmails/{id}/browser*` and the viewer's
 * socket. The page is a made-up phone screen whose colour moves on each tap; tabs, navigation and
 * dialogs answer as the backend's would (navigate to a URL holding `prompt` and the page asks).
 * A film under 1 KiB is refused as ffmpeg would a silent one. A Multilogin cloud phone draws its
 * model, has no tabs, its frames in screen px, its film in "the Library" whether it runs or not,
 * and a stream link; Multilogin's browser takes no film. Every change is told on the live socket
 * as `browsers`.
 *
 *   STUB_BROWSERS=off   every browser route answers 503 "browsers not connected"
 */
import type { IncomingMessage, ServerResponse } from "node:http";
import type { Duplex } from "node:stream";
import { crc32, deflateSync } from "node:zlib";

import { DEVICES, PROVIDERS, type Browser, type BrowserIn, type BrowserOut, type Tab } from "../src/entities/gmb/model/generated.ts";
import { changed, closing, frame, handshake, text } from "./stub-live.ts";

const CONNECTED = process.env.STUB_BROWSERS !== "off";
const CSS = { width: 412, height: 915 };
const PHONE = { width: 1080, height: 2400 };
const PX = 2; // the stub draws at half the CSS size: a phone's page is never sharper than it needs to be here

type Json = Record<string, unknown>;
interface Running {
  gmb: number | null;
  tabs: Tab[];
  active: string;
  hue: number;
  viewer: Duplex | null;
}
const browsers = new Map<number, Browser>();
const running = new Map<number, Running>();

/** The `browser` of a gmail's connections, and whether its must-have holds. */
export function browserOf(gmail: number): { browser: Browser | null; ready: boolean } {
  const b = browsers.get(gmail) ?? null;
  const on = running.get(gmail);
  const stream = b?.provider === "multilogin" ? `https://cloud-phone.multilogin.example/${gmail}` : null;
  return { browser: b && { ...b, running: on ? { gmb: on.gmb, stream } : null }, ready: b !== null && b.checked.error === null };
}

/** `/gmails/{id}/browser*`; `false`: not one of them. */
export async function browserRoute(req: IncomingMessage, res: ServerResponse, path: string, body: () => Promise<Json>, send: (res: ServerResponse, status: number, body?: unknown) => void, connections: (gmail: number) => unknown): Promise<boolean> {
  const m = path.match(/^\/gmails\/(\d+)\/browser(?:\/(test|start|stop|film))?$/);
  if (!m) return false;
  const gmail = Number(m[1]);
  const answer = (status: number, out?: unknown) => {
    if (status < 300 && req.method !== "GET") changed("browsers", null, String(gmail));
    send(res, status, out);
    return true;
  };
  const action = m[2] ?? null;
  if (!CONNECTED) return answer(503, { error: "browsers not connected" });
  const now = new Date().toISOString();
  if (action === null && req.method === "PUT") {
    const { provider, device, proxy } = (await body()) as { provider: string; device: string | null; proxy: string };
    const phone = provider === "multilogin";
    if (!(PROVIDERS as readonly string[]).includes(provider)) return answer(400, { error: "unknown provider" });
    if (phone ? device !== null : !(DEVICES as readonly string[]).includes(device ?? "")) return answer(400, { error: phone ? "a cloud phone's model is drawn: send no device" : "unknown device" });
    const set = browsers.get(gmail);
    if (set && (set.provider !== provider || (!phone && set.device !== device))) return answer(409, { error: "the provider and device are set once: remove the browser first" });
    const url = /^(socks5|http):\/\/[^@\s]+@([^:/\s]+):(\d+)$/.exec(proxy);
    if (!url) return answer(400, { error: "the proxy is socks5:// or http://, user:pass@host:port" });
    if (url[2]!.includes("us")) return answer(400, { error: "the proxy exits in US, outside FR where this gmail's GBPs are" });
    browsers.set(gmail, {
      provider: provider as Browser["provider"],
      device: set?.device ?? (phone ? "Google Pixel 7" : device!),
      endpoint: `${url[2]}:${url[3]}`,
      exit: { ip: "92.184.1.2", country: "FR", city: "Lyon", zone: "Europe/Paris", mobile: true },
      set_at: set?.set_at ?? now,
      checked: { at: now, error: null },
      running: null,
      film: set?.film ?? false,
    });
    return answer(200, connections(gmail));
  }
  const set = browsers.get(gmail);
  if (!set) return answer(404, { error: "not found" });
  if (action === null && req.method === "DELETE") {
    stop(gmail);
    browsers.delete(gmail);
    return answer(204);
  }
  if (action === "test" && req.method === "POST") {
    set.checked = { at: now, error: null };
    return answer(200, connections(gmail));
  }
  if (action === "start" && req.method === "POST") {
    const { gmb } = (await body()) as { gmb: number | null };
    const on = running.get(gmail);
    if (on && on.gmb !== gmb) return answer(409, { error: "it runs for another GBP, or for the gmail alone: stop it first" });
    const home: Tab = { id: "t1", title: "Google Business Profile", url: "https://business.google.com/create" };
    const phone = set.provider === "multilogin";
    if (!on) running.set(gmail, { gmb, tabs: phone ? [] : [home], active: phone ? "" : home.id, hue: 210, viewer: null });
    return answer(204);
  }
  if (action === "stop" && req.method === "POST") {
    stop(gmail);
    return answer(204);
  }
  if (action === "film" && req.method === "PUT") {
    if (req.headers["content-type"] !== "video/mp4") return answer(400, { error: "a film is video/mp4" });
    if (set.provider === "multilogin_browser") return answer(409, { error: "Multilogin's browser plays no film: its camera is the host's" });
    let size = 0;
    for await (const c of req) size += (c as Buffer).length;
    if (set.provider === "multilogin") {
      if (size > 100 * 1024 * 1024) return answer(413, { error: "a phone's film is 100 MiB at most" });
      set.film = true;
      return answer(204);
    }
    if (running.has(gmail)) return answer(409, { error: "stop it first: the next launch plays the film" });
    if (size < 1024) return answer(400, { error: "the film has no sound: a phone's mic is never silent" });
    set.film = true;
    return answer(204);
  }
  if (action === "film" && req.method === "DELETE") {
    set.film = false;
    return answer(204);
  }
  return answer(404, { error: "not found" });
}

function stop(gmail: number): void {
  const on = running.get(gmail);
  running.delete(gmail);
  on?.viewer?.end(closing(4410));
}

/** The viewer's socket: the last one to open takes the page from the one before. */
export function browserUpgrade(req: IncomingMessage, socket: Duplex, gmail: number): void {
  if (!handshake(req, socket)) return;
  const on = running.get(gmail);
  if (!on) return void socket.end(closing(4404));
  on.viewer?.end(closing(4409));
  on.viewer = socket;
  const out = (o: BrowserOut) => socket.write(text(o));
  const tabs = () => out({ type: "tabs", tabs: on.tabs, active: on.active });
  const space = browsers.get(gmail)?.provider === "multilogin" ? PHONE : CSS;
  const draw = () => out({ type: "frame", data: png(on.hue).toString("base64"), width: space.width, height: space.height });
  tabs();
  draw();
  const ticker = setInterval(draw, 1000);
  let pending = Buffer.alloc(0);
  socket.on("data", (chunk: Buffer) => {
    pending = Buffer.concat([pending, chunk]);
    for (let read = unmask(pending); read !== null; read = unmask(pending)) {
      pending = pending.subarray(read.size);
      if (read.opcode === 0x8) return void socket.end(closing(1000));
      if (read.opcode === 0x9) socket.write(frame(0xa, Buffer.alloc(0)));
      if (read.opcode !== 0x1) continue;
      const msg = JSON.parse(read.payload.toString()) as BrowserIn;
      const tab = on.tabs.find((t) => t.id === on.active);
      switch (msg.type) {
        case "touch":
          if (msg.phase === "end") {
            on.hue = (on.hue + 40) % 360;
            draw();
          }
          break;
        case "text":
        case "key":
          console.log(`browser ${gmail}:`, JSON.stringify(msg));
          break;
        case "navigate":
          if (tab) Object.assign(tab, { url: msg.url, title: new URL(msg.url).hostname });
          tabs();
          if (msg.url.includes("prompt")) out({ type: "dialog", kind: "prompt", message: "The page asks for a name", default: "stub" });
          break;
        case "activate":
          on.active = msg.id;
          tabs();
          break;
        case "new_tab": {
          const made = { id: `t${Date.now()}`, title: "New tab", url: "about:blank" };
          on.tabs.push(made);
          on.active = made.id;
          tabs();
          break;
        }
        case "close_tab":
          on.tabs = on.tabs.filter((t) => t.id !== msg.id);
          if (on.active === msg.id) on.active = on.tabs[0]?.id ?? "";
          tabs();
          break;
        case "dialog":
          console.log(`browser ${gmail}: dialog`, JSON.stringify(msg));
          out({ type: "dialog_closed" });
          break;
        case "history":
        case "wheel":
          break;
      }
    }
  });
  const forget = () => {
    clearInterval(ticker);
    if (on.viewer === socket) on.viewer = null;
  };
  socket.on("close", forget);
  socket.on("error", forget);
}

/** A client frame, masked as browsers send them; `null` until all of it is in. */
function unmask(buf: Buffer): { opcode: number; payload: Buffer; size: number } | null {
  if (buf.length < 2) return null;
  const opcode = buf[0]! & 0x0f;
  let len = buf[1]! & 0x7f;
  let at = 2;
  if (len === 126) {
    if (buf.length < 4) return null;
    len = buf.readUInt16BE(2);
    at = 4;
  } else if (len === 127) throw new Error("stub browser: a client frame over 64 KiB");
  if (buf.length < at + 4 + len) return null;
  const mask = buf.subarray(at, at + 4);
  const payload = Buffer.from(buf.subarray(at + 4, at + 4 + len).map((b, i) => b ^ mask[i % 4]!));
  return { opcode, payload, size: at + 4 + len };
}

/** A phone screen: a coloured bar over a light page with one field and one button. */
function png(hue: number): Buffer {
  const w = CSS.width / PX;
  const h = Math.round(CSS.height / PX);
  const [r, g, b] = hsl(hue);
  const rows = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++) {
    const row = y * (w * 3 + 1);
    for (let x = 0; x < w; x++) {
      const field = y > 120 && y < 145 && x > 20 && x < w - 20;
      const button = y > 170 && y < 192 && x > 20 && x < 90;
      const c = y < 40 || button ? [r, g, b] : field ? [255, 255, 255] : [236, 238, 242];
      rows.set(c, row + 1 + x * 3);
    }
  }
  const chunk = (type: string, data: Buffer) => {
    const head = Buffer.alloc(8);
    head.writeUInt32BE(data.length, 0);
    head.write(type, 4, "ascii");
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(Buffer.concat([head.subarray(4), data])), 0);
    return Buffer.concat([head, data, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr.set([8, 2, 0, 0, 0], 8);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(rows)), chunk("IEND", Buffer.alloc(0))]);
}

function hsl(hue: number): [number, number, number] {
  const f = (n: number) => {
    const k = (n + hue / 30) % 12;
    return Math.round(255 * (0.5 - 0.35 * Math.max(-1, Math.min(k - 3, 9 - k, 1))));
  };
  return [f(0), f(8), f(4)];
}
