This directory contains the hosting implementation for running wasm output.

See https://neugierig.org/software/blog/2026/05/theseus-wasm.html for an
overview.

## Running a target

Build the wasm for a target, then serve this directory:

```
bash build-wasm.sh --release winpin
(cd web && go run static-server.go)
```

The server sets the cross-origin isolation headers that SharedArrayBuffer
needs, and accepts two POSTs that make a page running under a script
observable: `/log` (messages, mirrored from the program's own log when the URL
carries `?frames=1`) and `/frame` (a PNG of the program's window).

A program with data files reads them from an in-memory filesystem that the page
fills in before starting it. Its entry in `PROGRAMS` in host.ts names a
`dataRoot` directory holding the files plus a `manifest.json` listing them, and
the `cwd` the program should start in. Files the program writes are kept in
localStorage and restored on the next load.

URL parameters: `?exe=<name>` picks a program from `PROGRAMS` (default `mine`),
`?frames=1` reports progress and window contents to the server, `?trace=<spec>`
traces winapi calls in the same syntax as `THESEUS_TRACE`.
