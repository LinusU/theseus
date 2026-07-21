// Note that the specific exe here doesn't matter, we just need the same types found in all of them.
import type * as exe from "./exe/basicdd/basicdd.js";

(self as any).send_to_host = (func: string, args: any[], retAddr: number) => {
  const obj: exe.Msg = { func, args, retAddr };
  self.postMessage(obj);
};

/// Load the program's data files into its in-memory filesystem.
///
/// The program reads files synchronously, so everything it might open has to
/// be resident before it starts; `manifest.json` lists the files to fetch.
async function mountData(exe: any, dataRoot: string, saves: Record<string, string>) {
  const manifest: string[] = await (await fetch(`${dataRoot}/manifest.json`)).json();
  let fetched = 0;
  const files = await Promise.all(
    manifest.map(async (name) => {
      const response = await fetch(`${dataRoot}/${name}`);
      if (!response.ok) throw new Error(`${name}: ${response.status}`);
      const data = new Uint8Array(await response.arrayBuffer());
      progress(`loading data ${++fetched}/${manifest.length}`);
      return [name, data] as const;
    }),
  );
  for (const [name, data] of files) {
    exe.mount_file(`/${name}`, data);
  }
  // Saved files from a previous session win over the shipped copies.
  for (const [name, encoded] of Object.entries(saves)) {
    const binary = atob(encoded);
    const data = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) data[i] = binary.charCodeAt(i);
    exe.mount_file(name, data);
  }
}

/// Show what the page is waiting on. Fetching a program and its data takes
/// long enough that a blank page reads as a hang.
function progress(message: string) {
  const msg: exe.Msg = { func: "loading", args: [message], retAddr: 0 };
  self.postMessage(msg);
}

/// Report to the server, which is the only way to see what a page is doing
/// when it's driven by a script rather than watched. Off otherwise: a page
/// served from anywhere else has nothing listening.
let reportToServer = false;

function report(message: string) {
  if (!reportToServer) return;
  fetch("/log", { method: "POST", body: message }).catch(() => {});
}

/// Mirror the program's log output to the server. The worker's console is out
/// of reach when the browser is driven by a script.
function mirrorConsole() {
  for (const level of ["log", "info", "warn", "error"] as const) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      original(...args);
      report(`${level}: ${args.join(" ")}`);
    };
  }
}

async function run(start: StartMessage) {
  reportToServer = start.mirrorConsole ?? false;
  if (start.mirrorConsole) mirrorConsole();
  report(`worker loading ${start.module}`);
  progress("loading program");
  const exe = await import(start.module);
  report("module imported");
  await exe.default(/* module */ undefined, start.memory);
  if (start.dataRoot) {
    report("loading data");
    await mountData(exe, start.dataRoot, start.saves ?? {});
    report("data loaded");
  }
  if (start.cwd) {
    exe.set_current_dir(start.cwd);
  }
  if (start.trace) {
    exe.set_trace(start.trace);
  }
  report("starting program");
  progress("starting");
  exe.main();
}

export interface StartMessage {
  module: string;
  memory: WebAssembly.Memory;
  /// URL prefix holding manifest.json and the program's data files.
  dataRoot?: string;
  /// Directory the program starts in, as a path within the mounted data.
  cwd?: string;
  /// Previously saved files, base64 encoded, keyed by mounted path.
  saves?: Record<string, string>;
  /// Send the program's log output to the server as well as the console.
  mirrorConsole?: boolean;
  /// Which winapi calls to trace, in THESEUS_TRACE syntax.
  trace?: string;
}

self.onmessage = (e: MessageEvent<StartMessage>) => {
  run(e.data).catch((e) => {
    report(`worker failed: ${e}\n${e?.stack ?? ""}`);
    progress(`failed: ${e}`);
    console.error(e);
  });
};
