#!/usr/bin/env node
// Records a click-through of a web flow in an already signed-in Chrome over CDP, as a webm.
// Chrome paints only a window that is on screen: under sway, park it on an output nobody looks at
// (`swaymsg create_output`, then move the window to that output's workspace).
//
//   node record.mjs --port 49300 --context <browserContextId prefix> --steps <steps.json> --out <file.webm> [name=value ...]
//
// steps.json: an array of
//   {nav: url} {wait: ms} {click: Find} {point: Find} {type: text, clear?: true} {enter: true}
//   {pause: true} {resume: true}            capture off / on (capture starts on)
//   {say: text}                             a caption pinned to the bottom until the next one; "" clears
// Find: {css?, tag?, text?, exact?, aria?, ph?, nth?}: the visible element matching, shortest text first.
// "{name}" in any string is replaced by the name=value arguments.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const args = process.argv.slice(2);
const opt = (k) => { const i = args.indexOf(`--${k}`); if (i < 0) throw new Error(`--${k} is required`); return args[i + 1]; };
const vars = Object.fromEntries(args.filter((a) => /^\w+=/.test(a)).map((a) => [a.slice(0, a.indexOf("=")), a.slice(a.indexOf("=") + 1)]));
const fill = (v) => (typeof v === "string" ? v.replace(/\{(\w+)\}/g, (m, k) => vars[k] ?? m) : Array.isArray(v) ? v.map(fill) : v && typeof v === "object" ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, fill(x)])) : v);
const steps = fill(JSON.parse(fs.readFileSync(opt("steps"), "utf8")));
const out = path.resolve(opt("out"));

const ver = await (await fetch(`http://127.0.0.1:${opt("port")}/json/version`)).json();
const ws = new WebSocket(ver.webSocketDebuggerUrl);
let id = 0;
const pend = new Map();
const frames = [];
let capturing = true;
let sess;
const send = (method, params = {}, sessionId) => new Promise((r) => { const i = ++id; pend.set(i, r); ws.send(JSON.stringify({ id: i, method, params, sessionId })); });
ws.onmessage = (e) => {
  const m = JSON.parse(e.data);
  if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); }
  if (m.method === "Page.screencastFrame") { if (capturing) frames.push([Date.now(), m.params.data]); send("Page.screencastFrameAck", { sessionId: m.params.sessionId }, sess); }
};
await new Promise((r) => (ws.onopen = r));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const ctx = opt("context");
const pages = (await send("Target.getTargets")).result.targetInfos.filter((t) => t.type === "page" && t.browserContextId.startsWith(ctx));
if (pages.length === 0) throw new Error(`no page in a browser context starting ${ctx}: open a tab in that profile`);
sess = (await send("Target.attachToTarget", { targetId: pages[0].targetId, flatten: true })).result.sessionId;
await send("Page.enable", {}, sess);
const ev = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true }, sess);
  if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description ?? JSON.stringify(r.result.exceptionDetails));
  return r.result.result.value ?? null;
};
const find = (q) => `(() => { const q = ${JSON.stringify(q)}; const vis = e => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
  const all = [...document.querySelectorAll(q.css || q.tag || 'button, a, [role=button], [role=option], [role=menuitem], [role=tab], [role=radio], [role=checkbox], label, li, span, div')];
  const m = all.filter(e => vis(e) && (!q.text || (q.exact ? e.innerText?.trim() === q.text : e.innerText?.includes(q.text))) && (!q.aria || (e.getAttribute('aria-label') || '').includes(q.aria)) && (!q.ph || (e.getAttribute('placeholder') || '').includes(q.ph)));
  m.sort((a, b) => (a.innerText?.length ?? 0) - (b.innerText?.length ?? 0));
  const e = m[q.nth || 0]; if (!e) return null; e.scrollIntoView({ block: 'center' }); const r = e.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2, w: r.width, h: r.height }; })()`;
const ring = (p) => ev(`(() => { const d = document.createElement('div'); d.style.cssText = 'position:fixed;z-index:2147483647;pointer-events:none;border:4px solid #ff2d55;border-radius:12px;box-shadow:0 0 0 6px rgba(255,45,85,.25);transition:opacity .3s;left:${p.x - p.w / 2 - 8}px;top:${p.y - p.h / 2 - 8}px;width:${p.w + 8}px;height:${p.h + 8}px'; document.body.appendChild(d); setTimeout(() => { d.style.opacity = 0; setTimeout(() => d.remove(), 400); }, 1100); })()`);
let caption = "";
const showCaption = () => caption && ev(`(() => { let c = document.getElementById('__rec_caption'); if (!c) { c = document.createElement('div'); c.id = '__rec_caption'; c.style.cssText = 'position:fixed;z-index:2147483647;pointer-events:none;left:50%;bottom:28px;transform:translateX(-50%);max-width:80vw;padding:10px 18px;border-radius:10px;background:rgba(17,17,17,.85);color:#fff;font:600 20px system-ui'; document.body.appendChild(c); } c.textContent = ${JSON.stringify(caption)}; })()`);
const key = (k, code, vk, modifiers = 0) => send("Input.dispatchKeyEvent", { type: "keyDown", key: k, code, windowsVirtualKeyCode: vk, modifiers }, sess).then(() => send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code, windowsVirtualKeyCode: vk, modifiers }, sess));

await send("Page.startScreencast", { format: "jpeg", quality: 85 }, sess);
for (const s of steps) {
  if (s.pause) capturing = false;
  if (s.resume) capturing = true;
  if (s.nav) { await send("Page.navigate", { url: s.nav }, sess); await sleep(4000); }
  if (s.wait) await sleep(s.wait);
  if (s.say !== undefined) { caption = s.say; if (!caption) await ev("document.getElementById('__rec_caption')?.remove()"); }
  await showCaption();
  const q = s.click ?? s.point;
  if (q) {
    let p = null;
    for (let i = 0; i < 30 && !p; i++) { p = await ev(find(q)); if (!p) await sleep(500); }
    if (!p) throw new Error(`not found: ${JSON.stringify(q)}`);
    await sleep(300);
    await ring(p);
    await sleep(900);
    if (s.click) for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) await send("Input.dispatchMouseEvent", { type, x: p.x, y: p.y, button: "left", clickCount: 1 }, sess);
    await sleep(s.after ?? 1500);
    await showCaption();
  }
  if (s.type !== undefined) {
    if (s.clear) { await key("a", "KeyA", 65, 2); await key("Backspace", "Backspace", 8); }
    for (const ch of s.type) { await send("Input.insertText", { text: ch }, sess); await sleep(60); }
    await sleep(500);
  }
  if (s.enter) { await key("Enter", "Enter", 13); await sleep(800); }
}
await sleep(1500);
await send("Page.stopScreencast", {}, sess);
ws.close();

// a frame lasts until the next one: Chrome sends one only when the page changes
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "record-tutorial-"));
const list = frames.map(([t, d], i) => { fs.writeFileSync(path.join(dir, `${i}.jpg`), Buffer.from(d, "base64")); const next = frames[i + 1]?.[0] ?? t + 1500; return `file '${i}.jpg'\nduration ${Math.min((next - t) / 1000, 4).toFixed(3)}`; });
fs.writeFileSync(path.join(dir, "list.txt"), list.join("\n") + `\nfile '${frames.length - 1}.jpg'\n`);
execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", "list.txt", "-vf", "scale=1280:-2,fps=15", "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "38", "-an", out], { cwd: dir });
fs.rmSync(dir, { recursive: true });
console.log(`${out}: ${frames.length} frames`);
