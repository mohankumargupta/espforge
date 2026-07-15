# ADR-012 — Networking & `http` component model (esp-radio + edge-http)

**Status:** accepted (decided 2026-07-16, grilling session)

## Context

v2 needs WiFi HTTP. The user planned to use `edge-http` (crates.io) and studied
how esphome models networking. Open question: is there a `tcp` component? How is
the TCP/IP stack wired? How does `http` relate to `esp32.wifi`?

Key facts established during the session:
- The v2 IR already carries cross-cutting flags `has_wifi` / `needs_stack` (not
  component kinds), and the emitter already adds `esp-wifi = "*"` /
  `embassy-net = "*"` to the generated Cargo.toml when those flags are set —
  but **no driver currently asserts `needs_stack`**, so the path is dormant.
- `edge_http::io::client::Connection::new(buf, stack, addr)` requires
  `stack: T: edge_nal::TcpConnect + edge_nal::Dns`. `embassy_net::Stack` does
  not implement those traits directly.
- The canonical esp-hal 1.1 example uses **`esp_radio::wifi`** (newer crate),
  builds one `Stack` singleton in `main` with two spawned tasks (`connection`,
  `net_task`).

## Decision

Model networking after **esphome**: a *global* network link, not a named
per-instance resource.

1. **No `tcp` component.** The TCP/IP `Stack` is implicit infrastructure, not a
   `components:` instance. This matches the existing IR flag design
   (`has_wifi`/`needs_stack`) and the design doc's software-service list
   (`http, mqtt, websockets, voice_control` — `tcp` absent).
2. **`esp32.wifi` is a top-level peripheral block** (already in schema:
   `ssid`/`password`/`auth`). It is **not claimed by any instance**. The emitter
   consumes it directly when `needs_stack` is set. `http` does **not** write
   `with: { bus: $wifi }`.
3. **`http` is a software-service `Component`** (`using: http`), asserting
   cross-cutting flags `is_embassy`, `has_wifi`, `needs_stack`, `has_alloc`.
4. **Stack built inline in `main.rs`** when `needs_stack`: one
   `embassy_net::Stack` + `StackResources`, spawned `connection` + `net_task`
   tasks, `wait_config_up()`, creds inlined from `esp32.wifi`. Exposed as a
   `&'static Stack` emitter-named global (`NET_STACK`).
5. **`Http` runtime wrapper** (`espforge-runtime::components::Http`) takes
   `&'static Stack` as an explicit ctor arg (`Http::new(NET_STACK)`) — same
   move-by-value convention as `ssd1306`/`i2c`. Internally wraps `edge_http`
   and hides buffer/read-loop boilerplate behind `async get/post ->
   Result<String>`. App never names `edge_http`/`Connection`/`Stack`.
6. **Bridge = `edge_nal_embassy`** (feature-gated runtime dep) provides the
   `edge_nal::TcpConnect + Dns` impl for `&embassy_net::Stack`.
7. **WiFi crate = `esp-radio` / `esp_radio::wifi`** (current maintained line,
   matches the canonical example). Supersedes the older `esp-wifi = "*"` line in
   `emit/rust.rs`. `has_wifi` stays the crate-agnostic flag name.
8. **Validation:** `validate` fails (span-aware `Diag`) if `http` present but
   `esp32.wifi` absent. `resolve` **auto-upgrades `runtime: blocking` → Embassy**
   when any instance asserts `is_embassy`. No blocking network path exists.
9. **Scope:** first implementation = **plaintext HTTP only** (port 80).
   TLS/HTTPS, `mqtt`, and `websockets` are **planned future work** (they reuse the
   same Stack singleton + `edge_nal` bridge; `edge-mqtt`, `edge-ws` / a ws crate +
   `rustls`/`esp-tls` for HTTPS) — deferred, not non-goals. UDP is out of scope.

## Consequences

- The per-instance peripheral-claim invariant (`claimed_by`, one claim per
  peripheral) is preserved — wifi is infrastructure, never an instance claim.
- Reuses the existing flag→dep mapping in the emit step; adds `esp-radio`,
  `edge-http`, `edge-nal`, `edge-nal-embassy` to the network dep set.
- Adds one open plumbing task: `is_embassy` must be added to `SpecFlags` and
  consulted by `resolve` (currently `DriverFlags.is_embassy` exists but the
  resolve loop doesn't read it).

## Alternatives considered

- **A: `tcp` as an explicit component, `http` lists `with: { component: $tcp }`.**
  Rejected: the Stack is a singleton; a named `tcp` instance invents a choice that
  doesn't exist and duplicates across `http`/`mqtt`/`websockets`.
- **B: `http` claims the `esp32.wifi` peripheral** (like `i2c` claims `I2C0`).
  Rejected: breaks `claimed_by` (singleton network resource owned by no instance)
  and contradicts esphome's global-link model.
- **C: keep `esp-wifi`** instead of `esp-radio`. Rejected: `esp-radio` is the
  maintained line and matches the only known-working reference example.
- **D: hand-write `edge_nal` adapter** for `embassy_net::Stack`. Rejected:
  re-implements the solved `edge-nal-embassy` glue.
- **E: stream `Response` reader to app.** Rejected for v1: pushes read-loop /
  buffer management into user code, the boilerplate espforge exists to remove.
