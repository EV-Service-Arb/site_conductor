import { appendFileSync, readFileSync, writeFileSync } from "node:fs";

import type { FullResult, Reporter, TestCase, TestResult } from "@playwright/test/reporter";

type Span = { name: string; start: number; end: number };
const WIDTH = 64;

/** The stack's phases (`local-stack`'s timeline.tsv) and the tests, on one axis, written to `out`. */
export default class Timeline implements Reporter {
  private readonly spans: Span[] = [];
  constructor(private readonly options: { stack: string; out: string }) {}

  onTestEnd(test: TestCase, result: TestResult) {
    const start = result.startTime.getTime();
    this.spans.push({ name: `${test.parent.project()?.name ?? "?"}: ${test.title}`, start, end: start + result.duration });
  }

  onEnd(_result: FullResult) {
    const stack: Span[] = readFileSync(this.options.stack, "utf8")
      .trim()
      .split("\n")
      .map((row) => {
        const [name, start, end] = row.split("\t");
        if (name === undefined || start === undefined || end === undefined) throw new Error(`timeline row ${JSON.stringify(row)}`);
        return { name, start: Number(start), end: Number(end) };
      });
    const spans = [...stack, ...this.spans].sort((a, b) => a.start - b.start);
    const t0 = Math.min(...spans.map((s) => s.start));
    const total = Math.max(...spans.map((s) => s.end)) - t0;
    const col = (t: number) => Math.round(((t - t0) / total) * WIDTH);
    const lines = spans.map((s) => {
      const from = col(s.start);
      const to = Math.max(col(s.end), from + 1);
      const bar = " ".repeat(from) + "█".repeat(to - from) + " ".repeat(WIDTH - to);
      return `${s.name.slice(0, 48).padEnd(48)} |${bar}| ${((s.end - s.start) / 1000).toFixed(1)}s`;
    });
    const text = [`total ${(total / 1000).toFixed(1)}s from the stack's first build`, ...lines].join("\n");
    writeFileSync(this.options.out, text + "\n");
    // GitHub renders a mermaid block in a job's summary; mermaid takes no ":" or ";" inside a task's name
    const task = (s: Span) => `${s.name.replace(/[:;#]/g, " ")} :${s.start - t0}, ${s.end - t0}`;
    const summary = [
      "<details><summary>e2e timeline</summary>",
      "",
      "```mermaid",
      "gantt",
      "  dateFormat x",
      "  axisFormat %M:%S",
      "  section stack",
      ...stack.map((s) => "  " + task(s)),
      "  section tests",
      ...this.spans.map((s) => "  " + task(s)),
      "```",
      "</details>",
      "",
    ].join("\n");
    writeFileSync(this.options.out.replace(/\.txt$/, ".md"), summary);
    if (process.env.GITHUB_STEP_SUMMARY !== undefined) appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
    console.log(`\n${text}\n(${this.options.out})`);
  }
}
