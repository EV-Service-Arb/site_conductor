import { api, ok } from "@/shared/api";
import type { Alerts, Destination } from "@/shared/api/schema";

/** This browser's subscription as it was last told to the server: whose, which destination, where. */
interface Kept {
  user: string;
  destination: number;
  endpoint: string;
}

const KEPT = "sa:push";

export function keptPush(): Kept | null {
  const raw = localStorage.getItem(KEPT);
  return raw === null ? null : (JSON.parse(raw) as Kept); // written below alone
}

/** Whether this browser takes pushes: "install", an iPhone's only once the panel is on its Home Screen. */
export function pushable(): "yes" | "install" | "no" {
  if ("serviceWorker" in navigator && "PushManager" in window && "Notification" in window) return "yes";
  const ios = /iPhone|iPad|iPod/.test(navigator.userAgent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
  return ios ? "install" : "no";
}

/** What the caller knows this browser by: "Chrome on Android". */
function browserName(): string {
  const ua = navigator.userAgent;
  const of = (names: readonly (readonly [RegExp, string])[], other: string) => names.find(([is]) => is.test(ua))?.[1] ?? other;
  const browser = of([[/Edg\//, "Edge"], [/OPR\//, "Opera"], [/Firefox\/|FxiOS/, "Firefox"], [/Chrome\/|CriOS/, "Chrome"], [/Safari\//, "Safari"]], "A browser");
  const system = of([[/Android/, "Android"], [/iPhone|iPod/, "iPhone"], [/iPad/, "iPad"], [/Windows/, "Windows"], [/Mac OS X/, "Mac"], [/Linux/, "Linux"]], "this computer");
  return `${browser} on ${system}`;
}

async function subscribed(key: string): Promise<PushSubscription> {
  const registration = await navigator.serviceWorker.register("/sw.js");
  await (await registration.pushManager.getSubscription())?.unsubscribe(); // one made for another key refuses a new one
  return registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key });
}

/** Subscribes this browser, asking its person when they were not yet; `null`: they refused. */
export async function subscribePush(user: string, key: string): Promise<Alerts | null> {
  if ((await Notification.requestPermission()) !== "granted") return null;
  await navigator.serviceWorker.register("/sw.js");
  await navigator.serviceWorker.ready;
  const { endpoint, keys } = (await subscribed(key)).toJSON();
  if (endpoint === undefined || keys?.p256dh === undefined || keys.auth === undefined) throw new Error("a push subscription without its keys");
  const alerts = await ok(api.POST("/alerts/push", { body: { endpoint, p256dh: keys.p256dh, auth: keys.auth, title: browserName() } }));
  const destination = Math.max(...alerts.destinations.filter((d) => d.method === "push").map((d) => d.id));
  localStorage.setItem(KEPT, JSON.stringify({ user, destination, endpoint } satisfies Kept));
  return alerts;
}

/** What keeps pushes off a browser its person left; its destination stays, dead, until they are back (`resubscribe`). */
export async function unsubscribePush(): Promise<void> {
  if (pushable() !== "yes") return;
  const registration = await navigator.serviceWorker.getRegistration("/");
  await (await registration?.pushManager.getSubscription())?.unsubscribe();
}

/** Subscribes anew a browser whose subscription is gone or moved (signed out of, dropped by the browser), as the destination it was. */
export async function resubscribe(user: string, alerts: Alerts): Promise<boolean> {
  const kept = keptPush();
  if (kept === null || kept.user !== user || alerts.push_key === null || pushable() !== "yes" || Notification.permission !== "granted") return false;
  const was: Destination | undefined = alerts.destinations.find((d) => d.id === kept.destination);
  if (was === undefined) {
    localStorage.removeItem(KEPT); // removed in the settings
    return false;
  }
  const registration = await navigator.serviceWorker.getRegistration("/");
  if ((await registration?.pushManager.getSubscription())?.endpoint === kept.endpoint) return false;
  const now = await subscribePush(user, alerts.push_key);
  if (now === null) return false;
  const id = keptPush()!.destination; // kept by the line above
  if (id !== was.id) {
    await ok(api.PUT("/alerts/destinations/{id}", { params: { path: { id } }, body: { title: was.title, cadences: was.cadences, rules: was.rules, filter: was.filter } }));
    await ok(api.DELETE("/alerts/destinations/{id}", { params: { path: { id: was.id } } }));
  }
  return true;
}
