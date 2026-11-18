// A session: one process whose cells run in turn, each a module the runner wrote and type-checked
// (sa_sandbox's main.rs). Its first line on stdin is a nonce; each next one a cell to run. What a
// cell prints is the cell's; the host's own word is a line on stdout behind the nonce.
import { setToken } from "./prelude.ts";

const lines = Deno.stdin.readable.pipeThrough(new TextDecoderStream()).getReader();
let buffered = "";
async function line(): Promise<string | null> {
  for (;;) {
    const at = buffered.indexOf("\n");
    if (at >= 0) {
      const l = buffered.slice(0, at);
      buffered = buffered.slice(at + 1);
      return l;
    }
    const { value, done } = await lines.read();
    if (done) return null;
    buffered += value;
  }
}

const nonce = await line();
if (nonce === null) Deno.exit(0);
const said = new TextEncoder();
const say = (word: unknown) => Deno.stdout.writeSync(said.encode(`\0SA:${nonce}:${JSON.stringify(word)}\n`));

// what a cell left running throws later is told, never the end of the session
globalThis.addEventListener("unhandledrejection", (e) => {
  e.preventDefault();
  console.error("Uncaught (in promise)", e.reason);
});
globalThis.addEventListener("error", (e) => {
  e.preventDefault();
  console.error("Uncaught", e.error);
});

for (let l = await line(); l !== null; l = await line()) {
  const { run, token } = JSON.parse(l) as { run: string; token: string };
  setToken(token);
  try {
    await import(run);
    say({ ended: "done" });
  } catch (e) {
    console.error(e);
    say({ ended: "threw" });
  }
}
// the runner is gone: what a cell left running goes with it
Deno.exit(0);
