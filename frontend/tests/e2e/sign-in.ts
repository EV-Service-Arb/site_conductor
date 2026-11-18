import { type FullConfig, request } from "@playwright/test";

import { SESSION } from "./playwright.config";

/** Dev sign-in: `/auth/login` goes straight to the callback, which opens a session; its cookies are every test's. */
export default async function signIn(config: FullConfig) {
  const baseURL = config.projects[0]?.use.baseURL;
  if (baseURL === undefined) throw new Error("no baseURL to sign in at");
  const context = await request.newContext({ baseURL });
  const answer = await context.get("/auth/login");
  if (!answer.ok()) throw new Error(`dev sign-in: ${answer.status()} at ${answer.url()}`);
  const me = await context.get("/api/v1/me");
  if (!me.ok()) throw new Error(`dev sign-in left no session: /api/v1/me answered ${me.status()}`);
  // `next dev` compiles a page on its first request: all at once here, not one worker's test timing out on each
  const warmed = await Promise.all(["/", "/alerts"].map((path) => context.get(path)));
  for (const page of warmed) if (!page.ok()) throw new Error(`warming ${page.url()}: ${page.status()}`);
  await context.storageState({ path: SESSION });
  await context.dispose();
}
