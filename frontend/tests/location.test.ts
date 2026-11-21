import { describe, expect, it, vi } from "vitest";

type Page = EventTarget & { location: { pathname: string; search: string }; history: Pick<History, "pushState" | "replaceState"> };
type Store = typeof import("@/shared/lib/location");

Object.assign(globalThis, { PopStateEvent: class extends Event {} });

/** A page loaded at `from` (its history as a browser keeps it, with nothing but what the store touches), after `act`, is at `to`. */
async function check(from: string, act: (store: Store) => void, to: string) {
  const page = new EventTarget() as Page;
  const at = (href: string) => {
    const url = new URL(href, "http://panel.test");
    page.location = { pathname: url.pathname, search: url.search };
  };
  at(from);
  page.history = { pushState: (_s, _t, url) => at(String(url)), replaceState: (_s, _t, url) => at(String(url)) };
  Object.assign(globalThis, { window: page });
  vi.resetModules();
  act(await import("@/shared/lib/location"));
  await Promise.resolve(); // the move's announcement
  expect(page.location.pathname + page.location.search).toBe(to);
}

describe("the location store", () => {
  it("keeps the carried keys across a move to another view", async () => {
    await check("/?gmbs=1,2&assistant=sidebar", (store) => store.go({ view: "tokens", params: {} }), "/user/tokens?assistant=sidebar&gmbs=1%2C2");
  });

  it("puts back the carried keys an element's own move dropped", async () => {
    await check("/sites?gmbs=3", (store) => {
        store.linkTo({ view: "home", params: {} }); // the page has rendered where it is
        window.history.pushState(null, "", "/sites/leads?stage=quoted");
      }, "/sites/leads?gmbs=3&stage=quoted");
  });

  it("takes a carried key out when told to, and does not put it back", async () => {
    await check("/user/tokens?gmbs=3&find=a", (store) => store.setCarried({ gmbs: undefined }), "/user/tokens?find=a");
  });

  it("sets Home's keys in place, or moves to Home with them", async () => {
    await check("/?gmb=5&tab=launch", (store) => store.setHome({ tab: undefined }), "/?gmb=5");
    await check("/user/alerts?find=x", (store) => store.setHome({ trash: true }), "/?find=x&trash=");
  });
});
