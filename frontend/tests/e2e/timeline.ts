import { appendFileSync, closeSync, existsSync, fstatSync, mkdirSync, openSync, readFileSync, readSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

import type { Reporter, TestCase, TestResult } from "@playwright/test/reporter";

/** `fn` as a span of the stack's timeline, beside its phases. */
export async function timed<T>(name: string, fn: () => Promise<T>): Promise<T> {
  const file = process.env.STACK_TIMELINE;
  if (file === undefined) throw new Error("STACK_TIMELINE: stackConfig sets it");
  const start = Date.now();
  appendFileSync(file, `${name}\t${start}\t\n`);
  const value = await fn();
  appendFileSync(file, `${name}\t${start}\t${Date.now()}\n`);
  return value;
}

/** `end` absent: still running. A `/` in `name` nests it under the span named by what precedes. */
type Span = { name: string; start: number; end: number | undefined };
type Row = { label: string; depth: number; start: number; end: number; open: boolean };

/** `$STACK_TIMELINE`'s rows: `name\tstart\t` as a span opens, `name\tstart\tend` as it closes. A last line without its newline is still being written. */
export function parse(text: string): Span[] {
  const spans = new Map<string, Span>();
  for (const row of text.slice(0, text.lastIndexOf("\n") + 1).split("\n").slice(0, -1)) {
    const cols = row.split("\t");
    const [name, start, end] = cols;
    if (cols.length !== 3 || !name || !/^\d+$/.test(start!) || !/^(\d+)?$/.test(end!)) throw new Error(`timeline row ${JSON.stringify(row)}`);
    const key = `${name}\t${start}`;
    const known = spans.get(key);
    if (end === "") {
      if (known) throw new Error(`timeline: ${name} opened twice at ${start}`);
      spans.set(key, { name, start: Number(start), end: undefined });
    } else {
      if (!known || known.end !== undefined) throw new Error(`timeline: ${name} at ${start} closed without being open`);
      if (Number(end) < known.start) throw new Error(`timeline: ${name} ends before it starts`);
      known.end = Number(end);
    }
  }
  return [...spans.values()];
}

/** Parents before their children, siblings by start. */
function tree(spans: Span[]): Span[] {
  const startOf = (name: string, before: number) =>
    Math.max(...spans.filter((s) => s.name === name && s.start <= before).map((s) => s.start), -Infinity);
  const key = (s: Span) => {
    const parts = s.name.split("/");
    return parts.flatMap((_, i) => {
      const name = parts.slice(0, i + 1).join("/");
      const at = startOf(name, s.start);
      return [at === -Infinity ? s.start : at, name];
    });
  };
  const keys = new Map(spans.map((s) => [s, key(s)]));
  return [...spans].sort((a, b) => {
    const [ka, kb] = [keys.get(a)!, keys.get(b)!];
    for (let i = 0; i < Math.min(ka.length, kb.length); i++) if (ka[i] !== kb[i]) return ka[i]! < kb[i]! ? -1 : 1;
    return ka.length - kb.length;
  });
}

export function duration(ms: number): string {
  const s = ms / 1000;
  if (s < 10) return `${s.toFixed(2)}s`;
  if (s < 60) return `${s.toFixed(1)}s`;
  return `${Math.floor(s / 60)}m${String(Math.floor(s % 60)).padStart(2, "0")}s`;
}

/** v_utils::chart::gantt's layout: label, duration, then the bar on an axis of `width` columns in all; times are ms from the origin. */
export function gantt(rows: Row[], width: number, axisEnd: number): string[] {
  if (axisEnd <= 0) throw new Error(`gantt axis must span time, got ${axisEnd}`);
  const labels = rows.map((r) => "  ".repeat(r.depth) + r.label);
  const lw = Math.min(Math.max(0, ...labels.map((l) => l.length)), Math.floor((width * 2) / 5));
  const bw = width - lw - 11;
  if (bw < 10) throw new Error(`width ${width} leaves ${bw} cells for bars`);
  const cell = (t: number) => Math.min(bw, Math.max(0, Math.round((t / axisEnd) * bw)));
  const step = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600].map((s) => s * 1000).find((s) => axisEnd / s <= bw / 8) ?? 3600_000;
  const axis = Array<string>(bw).fill(" ");
  for (let t = 0; t <= axisEnd; t += step) {
    const label = `|${duration(t).replace(/\.0+s$/, "s")}`;
    const at = cell(t);
    if (at + label.length <= bw) axis.splice(at, label.length, ...label);
  }
  const lines = [" ".repeat(lw + 11) + axis.join("")];
  rows.forEach((r, i) => {
    const cells = Array<string>(bw).fill(" ");
    const a = Math.min(cell(r.start), bw - 1);
    const b = Math.max(cell(r.end), a + 1);
    cells.fill(r.open ? "▓" : "█", a, b);
    if (r.open) cells[b - 1] = "▶";
    lines.push(`${labels[i]!.slice(0, lw).padEnd(lw)} ${duration(r.end - r.start).padStart(8)}  ${cells.join("").trimEnd()}`);
  });
  return lines;
}

type Test = Span & { project: string };
type Write = typeof process.stdout.write;

/** The last `n` lines of `file` as a terminal would leave them: colours dropped, a `\r` overwriting what preceded it. */
function tail(file: string, n: number): string[] {
  const fd = openSync(file, "r");
  const size = fstatSync(fd).size;
  const buf = Buffer.alloc(Math.min(size, 64 * 1024));
  readSync(fd, buf, 0, buf.length, size - buf.length);
  closeSync(fd);
  return buf
    .toString("utf8")
    .replace(/\x1b\[[0-9;?]*[A-Za-z]/g, "")
    .split("\n")
    .map((l) => l.slice(l.lastIndexOf("\r", l.length - 2) + 1).replace(/\r$/, ""))
    .slice(-n - 1, -1);
}

/** The stack's spans (`local-stack`'s `$STACK_TIMELINE`) and the tests on one axis. On a terminal the chart owns the screen, the run's own output going to `playwright.log` beside the stack's timeline (its tail shown under the chart); elsewhere a line every 30s. At the end (Ctrl-C too), the whole chart into `out` and a mermaid gantt into the job's summary. */
export default class Timeline implements Reporter {
  private readonly out: string;
  private readonly file: string;
  private readonly loaded = Date.now();
  private readonly tests = new Map<string, Test>();
  private timer: ReturnType<typeof setInterval> | undefined;
  /** on a terminal: where the output goes instead, and the terminal's own writes, restored at the end */
  private screen: { log: string; stdout: Write; stderr: Write } | undefined;
  private failure: unknown;
  private ended = false;

  /** v2: a v1 reporter is not told onConfigure, and its onBegin comes only after the webServer is up */
  version() {
    return "v2" as const;
  }

  printsToStdio() {
    return true;
  }

  constructor(options: { out: string }) {
    this.out = options.out;
    const file = process.env.STACK_TIMELINE;
    if (file === undefined) throw new Error("STACK_TIMELINE: stackConfig sets it");
    this.file = file;
  }

  onConfigure() {
    if (this.timer !== undefined) throw new Error("timeline: configured twice");
    mkdirSync(dirname(this.file), { recursive: true });
    if (process.stdout.isTTY) {
      const log = this.file.replace(/[^/]*$/, "playwright.log"); // not beside `out`: playwright empties its outputDir after configuring the reporters
      mkdirSync(dirname(log), { recursive: true });
      writeFileSync(log, "");
      this.screen = { log, stdout: process.stdout.write.bind(process.stdout), stderr: process.stderr.write.bind(process.stderr) };
      // every writer in this process (the list reporter, the webServer's lines, the workers' relayed output) into the log: the chart is the terminal's only writer
      const sink = (chunk: string | Uint8Array, encoding?: unknown, done?: unknown) => {
        appendFileSync(log, chunk);
        const callback = typeof encoding === "function" ? encoding : done;
        if (typeof callback === "function") callback();
        return true;
      };
      process.stdout.write = sink as Write;
      process.stderr.write = sink as Write;
      this.screen.stdout("\x1b[?1049h\x1b[?25l");
      this.timer = setInterval(() => this.live(() => this.draw()), 250);
    } else {
      this.timer = setInterval(() => this.live(() => process.stdout.write(this.snapshot() + "\n")), 30_000);
    }
    this.timer.unref();
    // a second Ctrl-C ends the runner before onEnd: the terminal and the chart still
    process.once("exit", () => {
      if (this.ended) return;
      this.stop();
      console.log(`\n${this.save()}\n(${this.out})`);
    });
  }

  onTestBegin(test: TestCase, result: TestResult) {
    this.tests.set(test.id + "#" + result.retry, { name: test.title, project: test.parent.project()?.name ?? "?", start: result.startTime.getTime(), end: undefined });
  }

  onTestEnd(test: TestCase, result: TestResult) {
    const t = this.tests.get(test.id + "#" + result.retry);
    if (!t) throw new Error(`test ${test.title} ended without beginning`);
    t.end = t.start + result.duration;
  }

  onEnd() {
    this.ended = true;
    const log = this.screen?.log;
    this.stop();
    if (this.failure !== undefined) throw this.failure;
    const text = this.save();
    const now = Date.now();
    const stack = this.stack();
    const t0 = Math.min(...stack.map((s) => s.start));
    // GitHub renders a mermaid block in a job's summary; mermaid takes no ":" or ";" inside a task's name
    const task = (s: Span) => `  ${s.name.replace(/[:;#]/g, " ")}${s.end === undefined ? " unfinished" : ""} :${s.start - t0}, ${(s.end ?? now) - t0}`;
    const summary = [
      "<details><summary>e2e timeline</summary>",
      "",
      "```mermaid",
      "gantt",
      "  dateFormat x",
      "  axisFormat %M:%S",
      "  section stack",
      ...tree(stack).map(task),
      "  section tests",
      ...[...this.tests.values()].map((t) => task({ ...t, name: `${t.project}: ${t.name}` })),
      "```",
      "</details>",
      "",
    ].join("\n");
    writeFileSync(this.out.replace(/\.txt$/, ".md"), summary);
    if (process.env.GITHUB_STEP_SUMMARY !== undefined) appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
    console.log(`\n${text}\n(${this.out})`);
    if (log !== undefined) console.log(`\n${tail(log, 20).join("\n")}\n(the run's output: ${log})`);
  }

  /** The whole chart, every span and test, into `out`; what is still running is marked unfinished. */
  private save(): string {
    const now = Date.now();
    const stack = this.stack();
    const tests = [...this.tests.values()].sort((a, b) => a.start - b.start).map((t) => ({ ...t, name: `${t.project}: ${t.name}` }));
    const t0 = Math.min(...stack.map((s) => s.start));
    const row = (s: Span): Row => {
      const parts = s.name.split("/");
      return { label: parts.at(-1)! + (s.end === undefined ? " (unfinished)" : ""), depth: parts.length - 1, start: s.start - t0, end: (s.end ?? now) - t0, open: s.end === undefined };
    };
    const width = process.stdout.isTTY ? process.stdout.columns - 1 : 160;
    const text = [`total ${duration(now - t0)} from the e2e's start`, ...gantt([...tree(stack), ...tests].map(row), width, now - t0)].join("\n");
    mkdirSync(dirname(this.out), { recursive: true });
    writeFileSync(this.out, text + "\n");
    return text;
  }

  /** What the stack wrote since this run began (a persistent stack's file holds earlier runs'), and the runner's own load. */
  private stack(): Span[] {
    const origin = Math.round(performance.timeOrigin);
    const written = existsSync(this.file) ? parse(readFileSync(this.file, "utf8")).filter((s) => s.start >= origin) : [];
    return [{ name: "playwright load", start: origin, end: this.loaded }, ...written];
  }

  private snapshot(): string {
    const now = Date.now();
    const stack = this.stack();
    const t0 = Math.min(...stack.map((s) => s.start));
    const all = [...stack, ...this.tests.values()];
    const open = all.filter((s) => s.end === undefined && !stack.some((o) => o.end === undefined && o.name.startsWith(s.name + "/")));
    return `[timeline ${duration(now - t0)}] running: ${open.map((s) => `${s.name} ${duration(now - s.start)}`).join(", ") || "nothing"} · done: ${all.length - all.filter((s) => s.end === undefined).length}`;
  }

  /** A broken timeline stops the live view, says so, and fails the run at its end: thrown from a timer, the runner would swallow it. */
  private live(tick: () => void) {
    try {
      tick();
    } catch (error) {
      this.stop();
      this.failure = error;
      process.stderr.write(`timeline: ${String(error)}\n`);
    }
  }

  /** The terminal back as it was: its own screen, cursor and writers. */
  private stop() {
    clearInterval(this.timer);
    if (this.screen === undefined) return;
    this.screen.stdout("\x1b[?25h\x1b[?1049l");
    process.stdout.write = this.screen.stdout;
    process.stderr.write = this.screen.stderr;
    this.screen = undefined;
  }

  /** The chart from the top of the screen, the log's tail under it. Finished nested spans, then finished tests (one row per project), then the oldest finished rows give way to its height. */
  private draw() {
    const screen = this.screen;
    if (screen === undefined) throw new Error("timeline: drawn off a terminal");
    const now = Date.now();
    const stack = tree(this.stack());
    const t0 = Math.min(...stack.map((s) => s.start));
    let rows: Row[] = stack.map((s) => {
      const parts = s.name.split("/");
      return { label: parts.at(-1)!, depth: parts.length - 1, start: s.start - t0, end: (s.end ?? now) - t0, open: s.end === undefined };
    });
    const tests = [...this.tests.values()].sort((a, b) => a.start - b.start);
    const projects = [...new Set(tests.map((t) => t.project))];
    const testRows: Row[] = projects.flatMap((p) => {
      const of = tests.filter((t) => t.project === p);
      const done = of.filter((t) => t.end !== undefined);
      const fold: Row[] = done.length === 0 ? [] : [{ label: `${p}: ${done.length}/${of.length} done`, depth: 0, start: done[0]!.start - t0, end: Math.max(...done.map((t) => t.end!)) - t0, open: false }];
      return [...fold, ...of.filter((t) => t.end === undefined).map((t) => ({ label: `${p}: ${t.name}`, depth: 1, start: t.start - t0, end: now - t0, open: true }))];
    });
    const { columns, rows: height } = process.stdout;
    const room = Math.floor(height * 0.7) - 2; // the header and the axis; the rest shows the log's tail
    let hidden = 0;
    if (rows.length + testRows.length > room) {
      const before = rows.length;
      rows = rows.filter((r) => r.open || r.depth === 0);
      hidden += before - rows.length;
    }
    while (rows.length + testRows.length > room) {
      const i = rows.findIndex((r) => !r.open);
      if (i === -1) break;
      rows.splice(i, 1);
      hidden++;
    }
    const header = `\x1b[1m▶ e2e ${duration(now - t0)}\x1b[0m${hidden > 0 ? ` (${hidden} finished hidden)` : ""}   output: ${screen.log}`;
    const chart = [header, ...gantt([...rows, ...testRows].slice(-room), columns - 1, now - t0), "─".repeat(columns - 1)];
    const lines = [...chart, ...tail(screen.log, Math.max(0, height - chart.length))].slice(0, height);
    screen.stdout("\x1b[H" + lines.map((l) => l.slice(0, columns - 1) + "\x1b[K").join("\r\n") + "\x1b[J");
  }
}
