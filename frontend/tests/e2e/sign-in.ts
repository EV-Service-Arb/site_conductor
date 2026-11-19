import { type FullConfig, request } from "@playwright/test";

/** Dev sign-in: `/auth/login` goes straight to the callback, which opens a session; its cookies are every test's. */
export default async function signIn(config: FullConfig) {
  const { baseURL, storageState } = config.projects[0]?.use ?? {};
  if (baseURL === undefined || typeof storageState !== "string") throw new Error("no baseURL to sign in at, or no storageState file to keep the session in");
  const context = await request.newContext({ baseURL });
  const answer = await context.get("/auth/login");
  if (!answer.ok()) throw new Error(`dev sign-in: ${answer.status()} at ${answer.url()}`);
  const me = await context.get("/api/v1/me");
  if (!me.ok()) throw new Error(`dev sign-in left no session: /api/v1/me answered ${me.status()}`);
  // `next dev` compiles a page on its first request: all at once here, not one worker's test timing out on each
  const warmed = await Promise.all(["/", "/alerts"].map((path) => context.get(path)));
  for (const page of warmed) if (!page.ok()) throw new Error(`warming ${page.url()}: ${page.status()}`);
  await context.storageState({ path: storageState });
  await context.dispose();
}
