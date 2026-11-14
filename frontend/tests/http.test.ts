import { describe, expect, it, vi } from "vitest";

import { createApi, ok } from "@/shared/api/client";
import { readCsrf } from "@/shared/api/csrf";
import { ApiError, createHttp } from "@/shared/api/http";

type Call = { url: string; init: RequestInit };

function harness(respond: (request: Request) => Response, cookie = "sa_csrf=tok-dev") {
  const requests: Request[] = [];
  const onUnauthenticated = vi.fn();
  const deps = {
    fetch: async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = new Request(input, init);
      requests.push(request);
      return respond(request);
    },
    cookie: () => cookie,
    onUnauthenticated,
  };
  return { api: createApi(deps, "http://panel.test/api/v1"), requests, onUnauthenticated };
}

function httpHarness(respond: () => Response) {
  const calls: Call[] = [];
  const http = createHttp({
    fetch: async (input, init) => {
      calls.push({ url: String(input), init: init ?? {} });
      return respond();
    },
    cookie: () => "sa_csrf=tok-dev",
    onUnauthenticated: () => {},
  });
  return { http, calls };
}

const json = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
const any = (v: unknown) => v;
const ME = { user_id: "00000000-0000-7000-8000-000000000001", email: "a@example.test", preferred_name: "A", permissions: [], dev_sign_in: false };

describe("the CSRF header", () => {
  it("goes on every write, from the sa_csrf cookie", async () => {
    const { api, requests } = harness((r) => (r.method === "POST" ? json(201, { key_id: "k", secret: "s" }) : new Response(null, { status: 204 })));
    await ok(api.POST("/sources", { body: { key_id: "k", kind: "site", brands: ["b"], origin: null } }));
    await ok(api.DELETE("/sources/{key_id}", { params: { path: { key_id: "k" } } }));
    expect(requests.map((r) => r.headers.get("x-sa-csrf"))).toEqual(["tok-dev", "tok-dev"]);
    expect(requests[0]?.headers.get("content-type")).toBe("application/json");
  });

  it("is not sent on reads", async () => {
    const { api, requests } = harness(() => json(200, ME));
    await ok(api.GET("/me"));
    expect(requests[0]?.headers.get("x-sa-csrf")).toBeNull();
  });

  it("prefers the __Host- cookie the backend sets behind https", () => {
    expect(readCsrf("sa_csrf=planted; __Host-sa_csrf=real; other=1")).toBe("real");
    expect(readCsrf("sa_csrf=dev")).toBe("dev");
    expect(readCsrf("__Host-sa_csrf=; x=1")).toBeNull();
    expect(readCsrf("")).toBeNull();
  });
});

describe("the init a write may add", () => {
  it("passes keepalive to fetch", async () => {
    const { http, calls } = httpHarness(() => new Response(null, { status: 204 }));
    await http.send("POST", "/auth/logout", undefined, any, undefined, { keepalive: true });
    expect(calls[0]?.init.keepalive).toBe(true);
  });

  it("leaves keepalive out when no init is given", async () => {
    const { http, calls } = httpHarness(() => new Response(null, { status: 204 }));
    await http.send("POST", "/auth/logout", undefined, any);
    expect(calls[0]?.init).not.toHaveProperty("keepalive");
  });

  it("cannot override the method, the headers or the CSRF header", async () => {
    const { http, calls } = httpHarness(() => json(201, {}));
    const hostile: object = { keepalive: true, method: "GET", headers: { "x-sa-csrf": "forged", accept: "text/plain" } };
    await http.send("POST", "/api/sites/leads", { a: 1 }, any, undefined, hostile);
    const headers = new Headers(calls[0]?.init.headers);
    expect(calls[0]?.init.method).toBe("POST");
    expect(headers.get("x-sa-csrf")).toBe("tok-dev");
    expect(headers.get("accept")).toBe("application/json");
    expect(calls[0]?.init.keepalive).toBe(true);
  });
});

describe("the gate's answers", () => {
  it("401 sends the browser to sign in", async () => {
    const { api, onUnauthenticated } = harness(() => json(401, { error: "sign in" }));
    await expect(ok(api.GET("/me"))).rejects.toMatchObject({ failure: { kind: "unauthenticated" } });
    expect(onUnauthenticated).toHaveBeenCalledOnce();
  });

  it("403 without a grant is 'no access', a 403 csrf is told apart", async () => {
    const noAccess = harness(() => json(403, { error: "no access to the panel" }));
    await expect(ok(noAccess.api.GET("/me"))).rejects.toMatchObject({ failure: { kind: "no_access" } });
    const csrf = harness(() => json(403, { error: "csrf" }));
    await expect(ok(csrf.api.DELETE("/sources/{key_id}", { params: { path: { key_id: "k" } } }))).rejects.toMatchObject({ failure: { kind: "csrf" } });
  });

  it("503 is 'unavailable' and signs nobody out", async () => {
    const { api, onUnauthenticated } = harness(() => json(503, { error: "sign-in is unavailable, try again" }));
    const err = await ok(api.GET("/me")).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).failure.kind).toBe("unavailable");
    expect(onUnauthenticated).not.toHaveBeenCalled();
  });
});
