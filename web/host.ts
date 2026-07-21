// Note that the specific exe here doesn't matter, we just need the same types found in all of them.
import type * as exe from "./exe/basicdd/basicdd.js";
import type * as worker from "./worker.js";


/// Prefix for saved files kept in localStorage.
const SAVE_PREFIX = "theseus:file:";

/// KeyboardEvent.code -> [PC set-1 scan code, Windows VK_*, extended key].
/// Mirrors the SDL host's table in host/src/sdl.rs; keys with no PC/AT
/// equivalent are simply absent.
const KEY_MAP: Record<string, [number, number, boolean]> = {
  Escape: [0x01, 0x1b, false],
  Digit1: [0x02, 0x31, false],
  Digit2: [0x03, 0x32, false],
  Digit3: [0x04, 0x33, false],
  Digit4: [0x05, 0x34, false],
  Digit5: [0x06, 0x35, false],
  Digit6: [0x07, 0x36, false],
  Digit7: [0x08, 0x37, false],
  Digit8: [0x09, 0x38, false],
  Digit9: [0x0a, 0x39, false],
  Digit0: [0x0b, 0x30, false],
  Minus: [0x0c, 0xbd, false],
  Equal: [0x0d, 0xbb, false],
  Backspace: [0x0e, 0x08, false],
  Tab: [0x0f, 0x09, false],
  KeyQ: [0x10, 0x51, false],
  KeyW: [0x11, 0x57, false],
  KeyE: [0x12, 0x45, false],
  KeyR: [0x13, 0x52, false],
  KeyT: [0x14, 0x54, false],
  KeyY: [0x15, 0x59, false],
  KeyU: [0x16, 0x55, false],
  KeyI: [0x17, 0x49, false],
  KeyO: [0x18, 0x4f, false],
  KeyP: [0x19, 0x50, false],
  BracketLeft: [0x1a, 0xdb, false],
  BracketRight: [0x1b, 0xdd, false],
  Enter: [0x1c, 0x0d, false],
  ControlLeft: [0x1d, 0xa2, false],
  KeyA: [0x1e, 0x41, false],
  KeyS: [0x1f, 0x53, false],
  KeyD: [0x20, 0x44, false],
  KeyF: [0x21, 0x46, false],
  KeyG: [0x22, 0x47, false],
  KeyH: [0x23, 0x48, false],
  KeyJ: [0x24, 0x4a, false],
  KeyK: [0x25, 0x4b, false],
  KeyL: [0x26, 0x4c, false],
  Semicolon: [0x27, 0xba, false],
  Quote: [0x28, 0xde, false],
  Backquote: [0x29, 0xc0, false],
  ShiftLeft: [0x2a, 0xa0, false],
  Backslash: [0x2b, 0xdc, false],
  KeyZ: [0x2c, 0x5a, false],
  KeyX: [0x2d, 0x58, false],
  KeyC: [0x2e, 0x43, false],
  KeyV: [0x2f, 0x56, false],
  KeyB: [0x30, 0x42, false],
  KeyN: [0x31, 0x4e, false],
  KeyM: [0x32, 0x4d, false],
  Comma: [0x33, 0xbc, false],
  Period: [0x34, 0xbe, false],
  Slash: [0x35, 0xbf, false],
  ShiftRight: [0x36, 0xa1, false],
  NumpadMultiply: [0x37, 0x6a, false],
  AltLeft: [0x38, 0xa4, false],
  Space: [0x39, 0x20, false],
  CapsLock: [0x3a, 0x14, false],
  F1: [0x3b, 0x70, false],
  F2: [0x3c, 0x71, false],
  F3: [0x3d, 0x72, false],
  F4: [0x3e, 0x73, false],
  F5: [0x3f, 0x74, false],
  F6: [0x40, 0x75, false],
  F7: [0x41, 0x76, false],
  F8: [0x42, 0x77, false],
  F9: [0x43, 0x78, false],
  F10: [0x44, 0x79, false],
  NumLock: [0x45, 0x90, false],
  ScrollLock: [0x46, 0x91, false],
  Numpad7: [0x47, 0x67, false],
  Numpad8: [0x48, 0x68, false],
  Numpad9: [0x49, 0x69, false],
  NumpadSubtract: [0x4a, 0x6d, false],
  Numpad4: [0x4b, 0x64, false],
  Numpad5: [0x4c, 0x65, false],
  Numpad6: [0x4d, 0x66, false],
  NumpadAdd: [0x4e, 0x6b, false],
  Numpad1: [0x4f, 0x61, false],
  Numpad2: [0x50, 0x62, false],
  Numpad3: [0x51, 0x63, false],
  Numpad0: [0x52, 0x60, false],
  NumpadDecimal: [0x53, 0x6e, false],
  F11: [0x57, 0x7a, false],
  F12: [0x58, 0x7b, false],
  // Extended keys: same scan code as their non-extended twin.
  NumpadEnter: [0x1c, 0x0d, true],
  ControlRight: [0x1d, 0xa3, true],
  NumpadDivide: [0x35, 0x6f, true],
  AltRight: [0x38, 0xa5, true],
  Home: [0x47, 0x24, true],
  ArrowUp: [0x48, 0x26, true],
  PageUp: [0x49, 0x21, true],
  ArrowLeft: [0x4b, 0x25, true],
  ArrowRight: [0x4d, 0x27, true],
  End: [0x4f, 0x23, true],
  ArrowDown: [0x50, 0x28, true],
  PageDown: [0x51, 0x22, true],
  Insert: [0x52, 0x2d, true],
  Delete: [0x53, 0x2e, true],
  MetaLeft: [0x5b, 0x5b, true],
  MetaRight: [0x5c, 0x5c, true],
  ContextMenu: [0x5d, 0x5d, true],
};

class MessageQueue {
  private messages: Event[] = [];
  private waiter: ((value: Event) => void) | undefined;

  poll(): Event | undefined {
    return this.messages.shift();
  }

  wait(): Promise<Event> {
    const msg = this.poll();
    if (msg !== undefined) {
      return Promise.resolve(msg);
    }
    const { promise, resolve } = Promise.withResolvers<Event>();
    this.waiter = resolve;
    return promise;
  }

  private enqueue = (e: Event) => {
    e.preventDefault();
    if (this.waiter) {
      this.waiter(e);
      this.waiter = undefined;
    } else {
      this.messages.push(e);
    }
  };
  private discard = (e: Event) => {
    e.preventDefault();
  };

  listen(dom: HTMLCanvasElement) {
    dom.onmousedown = this.enqueue;
    dom.onmouseup = this.enqueue;
    dom.onmousemove = this.enqueue;
    dom.oncontextmenu = this.discard;
    // Keys go to the document: a canvas only receives them when focused, and
    // games expect to be typed at as soon as they are on screen.
    document.addEventListener("keydown", this.enqueue);
    document.addEventListener("keyup", this.enqueue);
  }
}

/// Audio contexts to start once the user interacts with the page. Browsers
/// refuse to play audio before a gesture, and a program that asks for sound
/// while loading — as games do — asks too early, so the request has to be
/// replayed on the first click or keypress. The listeners stay installed
/// because a context can be suspended again later, and resuming one that is
/// already running does nothing.
const audioContexts: Set<AudioContext> = new Set();
let gestureHooked = false;

function resumeOnGesture(ctx: AudioContext) {
  audioContexts.add(ctx);
  if (gestureHooked) return;
  gestureHooked = true;
  const resumeAll = () => {
    for (const ctx of audioContexts) ctx.resume().catch(() => {});
  };
  for (const event of ["pointerdown", "keydown", "touchend"]) {
    document.addEventListener(event, resumeAll);
  }
}

/// Plays the samples the program's mixer produces, scheduling them back to
/// back so playback is continuous.
class AudioStream {
  private ctx: AudioContext;
  /// When the next buffer should start, in the context's timebase.
  private nextStart = 0;

  constructor(private sampleRate: number, private channels: number) {
    this.ctx = new AudioContext({ sampleRate });
    resumeOnGesture(this.ctx);
  }

  /// Samples handed over but not played yet, in bytes, which is what the
  /// program's mixer paces itself against.
  queued(): number {
    const ahead = Math.max(0, this.nextStart - this.ctx.currentTime);
    return Math.round(ahead * this.sampleRate) * this.channels * 2;
  }

  write(samples: Int16Array) {
    const frames = samples.length / this.channels;
    if (frames === 0) return;
    const buffer = this.ctx.createBuffer(this.channels, frames, this.sampleRate);
    for (let channel = 0; channel < this.channels; channel++) {
      const out = buffer.getChannelData(channel);
      for (let i = 0; i < frames; i++) {
        out[i] = samples[i * this.channels + channel]! / 32768;
      }
    }
    const source = this.ctx.createBufferSource();
    source.buffer = buffer;
    source.connect(this.ctx.destination);
    // Never schedule in the past: after a gap, restart from now.
    this.nextStart = Math.max(this.nextStart, this.ctx.currentTime);
    source.start(this.nextStart);
    this.nextStart += buffer.duration;
  }

  resume() {
    // Browsers only allow audio after a user gesture; retrying on each resume
    // means playback starts as soon as the player interacts with the page.
    this.ctx.resume().catch(() => {});
  }
}

class Host implements exe.WasmHost {
  consoleDom = document.createElement("pre");
  consoleOutput = new ArrayBuffer(0, { maxByteLength: 10 << 10 });
  window_: HTMLCanvasElement | undefined;

  surfaces: Map<number, HTMLCanvasElement> = new Map();
  nextSurface = 1;
  audioStreams: Map<number, AudioStream> = new Map();
  nextAudioStream = 1;
  messageQueue = new MessageQueue();

  loadingDom = document.getElementById("loading");

  constructor(public wasmMemory: WebAssembly.Memory) {
    this.consoleDom.id = "console";
    document.body.appendChild(this.consoleDom);
  }

  /// Progress from the worker, which does the slow part of startup.
  loading(message: string) {
    if (this.loadingDom) this.loadingDom.textContent = message;
  }

  onMessage(e: MessageEvent<exe.Msg>) {
    const msg = e.data;
    const ret = (this as any)[msg.func](...msg.args);
    if (msg.retAddr) {
      if (ret instanceof Promise) {
        ret.then((ret) => this.finishSync(msg.retAddr, ret));
        return;
      }
      this.finishSync(msg.retAddr, ret);
    }
  }

  finishSync(retAddr: number, ret: number | number[]): void {
    const arr = Array.isArray(ret) ? ret : [ret];
    if (!Number.isFinite(arr[0]) || arr[0] == 0) {
      // For synchronization to work, we must put a non-zero value in the first slot.
      // If this hits we messed up the sync/non-sync ness of some API.
      throw new Error();
    }
    const ints = new Int32Array(this.wasmMemory.buffer, retAddr, arr.length);
    ints.set(arr);
    Atomics.notify(ints, 0, 1);
  }

  console_write(ptr: number, len: number): void {
    const inBuf = new Uint8Array(this.wasmMemory.buffer, ptr, len);
    const ofs = this.consoleOutput.byteLength;
    this.consoleOutput.resize(ofs + len);
    const outBuf = new Uint8Array(this.consoleOutput, ofs, len);
    outBuf.set(inBuf);
    this.consoleDom.innerText = new TextDecoder().decode(this.consoleOutput);
  }

  create_surface(width: number, height: number): number {
    // Deliberately not added to the document: these are the program's
    // offscreen buffers, which reach the screen only when it draws one into
    // the window. Attaching them would show every back buffer below the game.
    const surface = document.createElement("canvas");
    surface.width = width;
    surface.height = height;

    const id = this.nextSurface++;
    this.surfaces.set(id, surface);
    return id;
  }

  create_window(title: string, width: number, height: number): number {
    // The program is up; whatever the page was waiting on is done.
    this.loadingDom?.remove();
    this.loadingDom = null;
    this.window_ = document.createElement("canvas");
    this.window_.className = "window";
    this.window_.width = width;
    this.window_.height = height;
    document.body.appendChild(this.window_);
    this.messageQueue.listen(this.window_);
    report(`window created ${width}x${height}`);
    return 1;
  }

  resize_window(id: number, width: number, height: number): void {
    this.window_!.width = width;
    this.window_!.height = height;
  }

  render(window_id: number, surface_id: number) {
    const surface = this.surfaces.get(surface_id)!;
    this.window_!.getContext("2d")!.drawImage(surface, 0, 0);
    reportFrame(this.window_!);
  }

  set_pixels(id: number, ptr: number, len: number): number {
    // TODO: investigate using OffscreenCanvas here instead,
    // https://news.ycombinator.com/item?id=48297805
    const copy = new Uint8ClampedArray(
      this.wasmMemory.buffer,
      ptr,
      len,
    ).slice();
    // The program's surfaces are opaque: the fourth byte of a pixel is the
    // unused X of XRGB, not alpha. Canvas would read it as transparency.
    for (let i = 3; i < copy.length; i += 4) copy[i] = 255;
    const surface = this.surfaces.get(id)!;
    const imageData = new ImageData(copy, surface.width);
    surface.getContext("2d")!.putImageData(imageData, 0, 0);
    return 1;
  }

  private serializeMessage(event: Event): number[] {
    // see wasm.rs:parse_message
    switch (event.type) {
      case "mousedown":
      case "mouseup":
      case "mousemove": {
        const typeToCode: Record<string, number> = {
          mousedown: 2,
          mouseup: 3,
          mousemove: 4,
        };
        const e = event as MouseEvent;
        // MouseEvent.button numbers left/middle/right 0/1/2, which lines up
        // with our bits, but MouseEvent.buttons orders them left/right/middle.
        const held = (e.buttons & 1) | ((e.buttons & 2) << 1) | ((e.buttons & 4) >> 1);
        return [typeToCode[e.type]!, e.offsetX, e.offsetY, (1 << e.button) | (held << 16)];
      }
      case "keydown":
      case "keyup": {
        const e = event as KeyboardEvent;
        const key = KEY_MAP[e.code];
        if (!key) return [];
        const [scancode, vkey, extended] = key;
        const flags = (extended ? 1 : 0) | (e.repeat ? 2 : 0);
        return [e.type === "keydown" ? 5 : 6, scancode, vkey, flags];
      }
      default:
        throw new Error();
    }
  }

  poll_message(): number[] {
    for (;;) {
      const event = this.messageQueue.poll();
      if (!event) return [-1];
      const msg = this.serializeMessage(event);
      // An empty result means a key we have no mapping for; skip it.
      if (msg.length) return msg;
    }
  }

  async wait_message(): Promise<number[]> {
    for (;;) {
      const event = await this.messageQueue.wait();
      const msg = this.serializeMessage(event);
      if (msg.length) return msg;
    }
  }

  create_audio_stream(sample_rate: number, channels: number): number {
    const id = this.nextAudioStream++;
    this.audioStreams.set(id, new AudioStream(sample_rate, channels));
    return id;
  }

  audio_queued(id: number): number {
    // Offset by one: the synchronization protocol reserves zero.
    return this.audioStreams.get(id)!.queued() + 1;
  }

  audio_write(id: number, ptr: number, len: number): number {
    const samples = new Int16Array(this.wasmMemory.buffer, ptr, len / 2).slice();
    this.audioStreams.get(id)!.write(samples);
    return 1;
  }

  audio_resume(id: number): number {
    this.audioStreams.get(id)!.resume();
    return 1;
  }

  write_file(path: string, ptr: number, len: number): number {
    const data = new Uint8Array(this.wasmMemory.buffer, ptr, len);
    let binary = "";
    for (const byte of data) binary += String.fromCharCode(byte);
    try {
      localStorage.setItem(SAVE_PREFIX + path, btoa(binary));
    } catch (e) {
      // Quota exceeded, private browsing, ...: losing a save beats crashing.
      console.warn("could not save", path, e);
    }
    return 1;
  }
}

/// Report a message to the server, which logs it. The browser console is not
/// always reachable — a page run from a script, a remote browser — and a game
/// that fails to start otherwise gives no clue why.
function report(message: string) {
  // Only when a script is watching: a page served from anywhere else has
  // nothing listening on /log.
  if (!REPORT_FRAMES) return;
  navigator.sendBeacon("/log", message);
}

/// With ?frames=1, post the program's window to the server as it draws, so a
/// script driving the browser can see the output. Driven by the program's own
/// rendering rather than a timer, which browsers throttle in hidden tabs.
const REPORT_FRAMES = location.search.includes("frames");
let lastFrameReport = 0;
let frameCount = 0;

function reportFrame(canvas: HTMLCanvasElement) {
  frameCount++;
  if (!REPORT_FRAMES) return;
  const now = Date.now();
  if (now - lastFrameReport < 3000) return;
  lastFrameReport = now;
  report(`frame ${frameCount}`);
  canvas.toBlob((blob) => {
    if (!blob) return;
    const reader = new FileReader();
    reader.onload = () => {
      const url = reader.result as string;
      fetch("/frame", { method: "POST", body: url.slice(url.indexOf(",") + 1) });
    };
    reader.readAsDataURL(blob);
  });
}

window.addEventListener("error", (e) => report(`error: ${e.message}`));
window.addEventListener("unhandledrejection", (e) => report(`rejected: ${e.reason}`));

// Programs to run, chosen with `?exe=`. One with data files also needs the
// directory holding them plus a manifest.json, and the directory to start in.
const PROGRAMS: Record<string, Omit<worker.StartMessage, "memory">> = {
  mine: { module: "./exe/mine/mine.js" },
  basicdd: { module: "./exe/basicdd/basicdd.js" },
  winpin: {
    module: "./exe/winpin/winpin.js",
    dataRoot: "./game",
    cwd: "/Soccer98",
  },
};

async function main() {
  if (!window.SharedArrayBuffer) {
    document.body.innerText = "SharedArrayBuffer is not supported; possibly try reloading";
    report("SharedArrayBuffer is not available");
    return;
  }

  const memory = new WebAssembly.Memory({
    // In units of 64KB pages. Has to cover the module's own declared minimum,
    // which grows with the size of the translated program.
    initial: 512, // 32mb
    // A translated game reserves a flat address space for itself and keeps its
    // data files in memory too, so leave room for both.
    maximum: 16384, // 1gb
    shared: true,
  });

  const host = new Host(memory);
  const worker = new Worker("./worker.js", { type: "module" });
  worker.onmessage = (e) => host.onMessage(e);

  // Files the program wrote in an earlier session.
  const saves: Record<string, string> = {};
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i)!;
    if (key.startsWith(SAVE_PREFIX)) {
      saves[key.slice(SAVE_PREFIX.length)] = localStorage.getItem(key)!;
    }
  }

  const params = new URLSearchParams(location.search);
  const name = params.get("exe") ?? "mine";
  const program = PROGRAMS[name];
  if (!program) {
    document.body.innerText = `no such program ${name}`;
    return;
  }

  const message: worker.StartMessage = {
    ...program,
    memory,
    saves,
    mirrorConsole: REPORT_FRAMES,
    trace: params.get("trace") ?? "",
  };
  worker.onerror = (e) => report(`worker: ${e.message}`);
  worker.postMessage(message);
  report(`started ${message.module}`);
}
main().catch((e) => console.error(e));
