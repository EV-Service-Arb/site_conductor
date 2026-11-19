import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, openSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { basename } from "node:path";

import type { FullConfig } from "@playwright/test";

import { timed } from "./timeline";

/** stackConfig's `metadata.stack`: `dev <argv>`, its state dir, where it answers, and what makes it current — its watched build catching up with the sources, or (--fixed) the copy it serves being the checkout as it is now */
export type Stack = { argv: string[]; env: Record<string, string>; state: string; web: string; health: string; fresh: "watched" | "fixed" };

const root = new URL("../../../", import.meta.url).pathname;
const budget = 30 * 60_000; // the conductor's cold build

const mtime = (path: string) => (existsSync(path) ? statSync(path).mtimeMs : -Infinity);
const tail = (path: string) => (existsSync(path) ? readFileSync(path, "utf8").split("\n").slice(-40).join("\n") : `(no ${path})`);
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const answers = (url: string) => fetch(url, { signal: AbortSignal.timeout(5_000) }).then((r) => r.ok, () => false);

/** local-stack's pid from its pidfile, while that process is still local-stack */
function alive(state: string): number | undefined {
  if (!existsSync(`${state}/pid`)) return undefined;
  const pid = Number(readFileSync(`${state}/pid`, "utf8"));
  try {
    return execFileSync("ps", ["-o", "args=", "-p", String(pid)], { encoding: "utf8" }).includes("local-stack") ? pid : undefined;
  } catch {
    return undefined; // ps exits 1 for a pid that is gone
  }
}

const gone = (pid: number) => {
  try {
    process.kill(pid, 0);
    return false;
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code !== "ESRCH") throw e;
    return true;
  }
};

/** The newest mtime among what `serving panel` watches: crates' rs/toml/sql/lock but crates/gen, and the workspace's manifest and lock. */
function newestSource(): number {
  let newest = Math.max(mtime(`${root}Cargo.toml`), mtime(`${root}Cargo.lock`));
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const path = `${dir}/${e.name}`;
      if (e.isDirectory()) {
        if (!["node_modules", "target"].includes(e.name) && !e.name.startsWith(".") && path !== `${root}crates/gen`) walk(path);
      } else if (/\.(rs|toml|sql|lock)$/.test(e.name)) newest = Math.max(newest, statSync(path).mtimeMs);
    }
  };
  walk(`${root}crates`);
  return newest;
}

/** The name local-stack gives --fixed's copy of the checkout as it is now: its `worktree_files | … | sha256sum`, kept in step with it. */
const checkoutHash = () =>
  execFileSync("bash", ["-c", `git ls-files -z -co --exclude-standard --deduplicate | while IFS= read -rd '' f; do [ ! -e "$f" ] || printf '%s\\0' "$f"; done | xargs -0 sha256sum | sha256sum | cut -c1-12`], { cwd: root, encoding: "utf8" }).trim();

/** A `dev` stack left running between runs: started when down, or stopped and started again when started with other args or no longer current; then waited on until it answers and, watched, until its build is no older than the sources. `nix run .#e2e -- --stop` / `.#figma -- --stop` stops it. */
export default async function stack(config: FullConfig) {
  const { argv, env, state, web, health, fresh } = config.metadata.stack as Stack;
  const expected = [...["--dev", ...argv], `SA_CONDUCTOR_CONFIG=${env.SA_CONDUCTOR_CONFIG ?? ""}`].join("\n") + "\n";
  let pid = alive(state);
  const stale = () => readFileSync(`${state}/args`, "utf8") !== expected || (fresh === "fixed" && basename(readFileSync(`${state}/src`, "utf8").trim()) !== checkoutHash());
  if (pid !== undefined && stale()) {
    process.kill(pid, "SIGTERM"); // local-stack's stop_all, which drops the pidfile first and exits once its parts have
    for (let i = 0; gone(pid) === false; i++) {
      if (i === 60) throw new Error(`local-stack ${pid} did not stop in 30s`);
      await sleep(500);
    }
    pid = undefined;
  }
  let died = () => alive(state) === undefined;
  if (pid === undefined) {
    mkdirSync(state, { recursive: true });
    for (const stamp of ["panel.built", "panel.failed"]) rmSync(`${state}/${stamp}`, { force: true }); // the last stack's, read below before local-stack clears them
    const log = openSync(`${state}/stack.log`, "w");
    const stackEnv: Record<string, string | undefined> = { ...process.env, ...env, GOLOGIN_TOKEN: undefined, MULTILOGIN_TOKEN: undefined }; // no spec opens the gmails' browsers: no runner, nor its builds
    const child = spawn("dev", argv, { cwd: root, detached: true, stdio: ["ignore", log, log], env: stackEnv }); // a group of its own, outliving this run
    child.unref();
    let exited = false;
    child.on("exit", () => (exited = true));
    died = () => exited;
  }
  const newest = fresh === "watched" ? newestSource() : -Infinity; // --fixed builds once, before it answers
  const failed = () => {
    const at = mtime(`${state}/panel.failed`);
    if (at >= newest && at > mtime(`${state}/panel.built`)) throw new Error(`the conductor did not build — the tail of ${state}/logs/panel.log:\n${tail(`${state}/logs/panel.log`)}`);
  };
  for (const start = Date.now(); !(await answers(web)); await sleep(500)) {
    failed();
    if (died()) throw new Error(`the stack stopped before ${web} answered — the tail of ${state}/stack.log:\n${tail(`${state}/stack.log`)}`);
    if (Date.now() - start > budget) throw new Error(`${web} did not answer in ${budget / 60_000} min`);
  }
  if (mtime(`${state}/panel.built`) >= newest && (await answers(health))) return;
  await timed("rebuild", async () => {
    for (const start = Date.now(); !(mtime(`${state}/panel.built`) >= newest && (await answers(health))); await sleep(250)) {
      failed();
      if (died()) throw new Error(`the stack stopped — the tail of ${state}/stack.log:\n${tail(`${state}/stack.log`)}`);
      if (Date.now() - start > budget) throw new Error(`the conductor's watched build did not catch up in ${budget / 60_000} min`);
    }
  });
}
