

## Document 1: `ota.md`

```markdown
# OTA Architecture & Implementation Plan (`espforge`)

This document specifies how to implement dual-slot A/B Over-The-Air (OTA)
firmware updates in `espforge` using
[`esp-hal-ota`](https://github.com/filipton/esp-hal-ota) (`0.7.2`) with an
**overlay-first** project model.

---

## 1. Architectural Decisions

### 1.1 `<ota />` Lives in `# espforge`
OTA partition tables, bootloader state, and flash-controller ownership are
system-wide firmware properties (like `<console>` and `<runtime>`), not a
single network protocol:

```xml
# espforge
  <project name="ota_demo" />
  <chip type="esp32c3" />
  <runtime type="embassy" />
  <ota flash_size="4MB" rollback="true" />
```

Why `# espforge`:
- **Transport-agnostic:** `esp-hal-ota` operates on `FlashStorage` synchronously
  with no heap allocations. Placing `<ota />` in `# espforge`
  allows OTA over HTTP, HTTPS, WebSocket, UART, BLE, or SD card, on both
  `blocking` and `embassy` runtimes.
- **Fits `FilePlan` and `cargo_config`:** `espforge-emit` already drives file
  planning (`plan.rs`) and `.cargo/config.toml` (`config.rs`) from
  `ResolvedOptions`. Making `ota` a derived option computed from `Meta::ota`
  matches `embassy` and `wifi` cleanly.
- **Matches `hold_runtime_resources`:** Just as `<console type="uart">` holds
  `UART0` and `<runtime type="embassy">` holds `TIMG0.timer0`, `<ota />` holds
  `FLASH` in `ResourceGraph`.

### 1.2 Powered by `esp-hal-ota` (`0.7.2`)
[`filipton/esp-hal-ota`](https://github.com/filipton/esp-hal-ota) (`0.7.2`,
MIT, compatible with `esp-hal ~1.2.2` and `esp-storage 0.10.0`) provides:
- Dynamic partition table discovery from flash (no compile-time macros needed).
- Streaming chunk writes (`Ota::begin(image_size, target_crc)` →
  `Ota::write(&chunk)` → `Ota::progress()` → `Ota::finish(enable_rollback)`).
- CRC32 verification and read-back verification before activating the new slot.
- Rollback support (`Ota::mark_app_valid()` after boot) and tolerance for
  erased/corrupt `otadata`.
- No heap allocations (`alloc` not required) and error-returning API (no panics).

### 1.3 Overlay-First Build Model (Removing `Ownership` / `manifest.json`)
Instead of tracking drift inside `build/` via `Ownership` (`Owned`, `Merged`,
`SeedOnce`), `.espforge/manifest.json`, and `ManifestDrift`:
- **All user-edited files live next to `<project>.espf`**, including optional
  overlays (`partitions.csv`, `rust-toolchain.toml`, `.cargo/config.toml`,
  `.cargo/esp-config.toml`, `Cargo.extra.toml`, `diagram.json`, `wokwi.toml`).
- **`build/` is 100% disposable generated output**, unconditionally overwritten on
  `espforge gen` / `espforge build` (just like `build/src/app.rs` and
  `chip.wasm` already are).
- This prevents `.cargo/config.toml` from getting stuck on stale `--chip`,
  `probe-rs`, or `--partition-table partitions.csv` flags when `.espf` changes.

### 1.4 Preserved `espforge` Invariants
- **No `static` storage in `espforge-runtime`** (`no_static_storage_in_runtime`
  test in `espforge-emit/src/codegen/rust.rs`). `OtaStorage` is declared as a
  `static` in generated `src/generated.rs` and passed into `Ota::new_in(...)`.
- **Host tests pass (`cargo test --workspace`):** Target crates (`esp-hal`,
  `esp-storage`, `esp-hal-ota`) remain strictly under
  `[target.'cfg(any(target_arch = "riscv32", target_arch = "xtensa"))'.dependencies]`.
- **Byte-identical non-OTA projects:** When `<ota />` is absent, `partitions.csv`
  is not emitted, `Context` does not gain an `ota` field, and `Cargo.toml` /
  `.cargo/config.toml` stay unchanged.

---

## 2. Crate-by-Crate Implementation Plan

### 2.1 `espforge-schema`

**File:** `espforge-schema/src/espf.schema.kdl`

1. Add the `<ota>` element under the `espforge` section:
   ```kdl
   element "ota" section="espforge" {
       summary "Enable dual-slot A/B Over-The-Air firmware updates"
       attr "flash_size" kind="choice" default="4MB" {
           choices "2MB" "4MB" "8MB" "16MB"
           help "Flash chip size used to generate the default partitions.csv layout."
       }
       attr "rollback" kind="bool" default="true" {
           help "Require ctx.ota.mark_app_valid() after boot, otherwise roll back on reset."
       }
       example "default 4MB with rollback" #"<ota />"#
       example "custom flash size" #"<ota flash_size="8MB" rollback="true" />"#
   }
   ```
2. Add the derived option so `FilePlan` and `cargo_config` can query `opts.is_on("ota")`:
   ```kdl
   option "ota" display_name="Enable OTA partition layout and updater." derived=#true {
       help "Derived from <ota /> in # espforge."
   }
   ```

---

### 2.2 `espforge-elements`

1. **`src/model.rs`**:
   Add `OtaConfig` to `Meta`:
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub struct OtaMeta {
       pub flash_size: FlashSize, // Mb2 | Mb4 | Mb8 | Mb16
       pub rollback: bool,
   }

   pub struct Meta {
       // ...
       pub ota: Option<OtaMeta>,
   }
   ```

2. **`src/core.rs`**:
   - Allow `"ota"` in `core_from_section` alongside `"project"`, `"chip"`,
     `"runtime"`, `"console"`, `"options"`.
   - Reject a second `<ota>` with `CORE_DUPLICATE_ELEMENT` (`E0203`).
   - Validate `flash_size` (`"2MB" | "4MB" | "8MB" | "16MB"`, default `"4MB"`)
     and `rollback` (`"true" | "false"`, default `true`) via generated
     `generated::ota::Attrs::from_node`.
   - Update the `CORE_UNKNOWN_ELEMENT` expected-list label to include `<ota>`.

3. **`src/options.rs`**:
   In `derived_flag(name, core, derived)`, compute `"ota"` from
   `core.ota.is_some()`:
   ```rust
   "ota" => {
       let on = core.ota.is_some();
       Some((
           on,
           None,
           if on {
               "derived from <ota /> in # espforge".into()
           } else {
               "off because there is no <ota /> in # espforge".into()
           },
       ))
   }
   ```

4. **`src/lower.rs`**:
   In `hold_runtime_resources(cx, meta)`:
   ```rust
   if meta.ota.is_some() {
       cx.hold(
           "FLASH",
           "OTA firmware updates (`<ota>`)",
           "remove `<ota />` from `# espforge` if another element needs raw FLASH access",
       );
   }
   ```

5. **`src/element.rs`**:
   Add `"ota"` to `RESERVED_IDENTS` so a user `<http_client id="ota" />` or
   `<gpio id="ota" />` produces `DEVICE_RESERVED_ID` / `SERVICE_RESERVED_ID`
   instead of shadowing `let ota = ...` or `ctx.ota`.

---

### 2.3 `espforge-runtime`

1. **`espforge-runtime/Cargo.toml`**:
   ```toml
   [features]
   ota = ["dep:esp-hal-ota", "dep:esp-storage"]

   [target.'cfg(any(target_arch = "riscv32", target_arch = "xtensa"))'.dependencies]
   esp-storage = { version = "0.10.0", features = ["embedded-storage"], optional = true }
   esp-hal-ota = { version = "0.7.2", features = ["log"], optional = true }
   ```
   *(Do **not** enable a chip feature here—the generated project's `Cargo.toml`
   supplies the chip feature through Cargo feature unification, matching
   `esp-hal` and `esp-radio`.)*

2. **New module `espforge-runtime/src/ota.rs`** (re-exported in `hw` when
   `feature = "ota"` is active):
   - **`OtaStorage`** (declared as `static __OTA_STORAGE` in generated
     `src/generated.rs`, never inside `espforge-runtime`):
     ```rust
     pub struct OtaStorage {
         inner: RefCell<Option<esp_hal_ota::Ota<esp_storage::FlashStorage<'static>>>>,
     }
     ```
     *(If `Context` is shared across Embassy tasks via `&'static Context`, wrap
     with `embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex` or
     `AsyncMutex` when `feature = "embassy"`, or keep single-owner access via
     `RefCell` / `Mutex`.)*
   - **`Ota`** handle stored on `Context`:
     - `pub fn new_in(storage: &'static OtaStorage, flash: FLASH<'static>, rollback: bool) -> Result<Self, OtaError>`
       Initializes `FlashStorage::new(flash)` → `esp_hal_ota::Ota::new(...)`
       inside `storage`.
     - `pub fn begin(&self, image_size: u32, target_crc32: u32) -> Result<(), OtaError>`
     - `pub fn write(&self, chunk: &[u8]) -> Result<bool, OtaError>` (returns
       `true` when all `image_size` bytes are written)
     - `pub fn progress(&self) -> Option<u8>` (percentage `0..=100` via
       `ota.progress()`)
     - `pub fn finish(&self) -> Result<(), OtaError>` (passes `self.rollback`
       to `ota.finish(self.rollback)` which runs CRC32 + flash read-back
       verification and updates `otadata`)
     - `pub fn mark_app_valid(&self) -> Result<(), OtaError>` (calls
       `ota.mark_app_valid()`)
     - `pub fn reboot(&self) -> !` (calls `esp_hal::system::software_reset()`)

3. **Streaming HTTP/HTTPS support in `espforge-runtime/src/services/`**:
   `HttpClient::get` and `HttpsClient::get` currently take `rx_buf: &mut [u8]`
   for the whole body. Add a streaming method on both clients:
   ```rust
   pub async fn get_stream<F, E>(
       &self,
       url: &str,
       mut on_chunk: F,
   ) -> Result<StreamInfo, HttpStreamError<E>>
   where
       F: FnMut(&[u8]) -> Result<(), E>;
   ```
   - `StreamInfo` returns `{ status: u16, content_length: Option<u32>, crc32_header: Option<u32> }`
     (reading optional `X-Firmware-CRC32` or letting the caller pass an
     expected CRC32 from a prior manifest GET before calling `ctx.ota.begin(...)`).
   - Streams in `2 KiB`–`4 KiB` stack/slot chunks so firmware of any size fits
     in SRAM.
   - *(Note: `edge-http` supports reading the response body incrementally via
     `connection.split()` and `response.read(&mut chunk_buf)`—`http_client.rs`
     already uses this loop, just into a single `rx_buf` instead of yielding
     chunks to a callback.)*

---

### 2.4 `espforge-core` (Overlay Discovery)

**File:** `espforge-core/src/source.rs`

Expand `OVERLAY_FILES` so users can customize any generated config next to `.espf`:
```rust
const OVERLAY_FILES: &[&str] = &[
    "diagram.json",
    "wokwi.toml",
    "chip.json",
    "chip.wasm",
    "partitions.csv",
    "rust-toolchain.toml",
    ".cargo/config.toml",
    ".cargo/esp-config.toml",
    "Cargo.extra.toml",
];
```

---

### 2.5 `espforge-emit`

1. **Remove `Ownership` / `manifest.rs` / `ManifestDrift`**:
   - Delete `espforge-emit/src/manifest.rs` and `ManifestDrift`.
   - Simplify `Artifact` / `Asset` (remove `Ownership`).
   - Update `espforge-emit/src/lib.rs::write` and `write_assets` to write
     every planned + overlaid artifact directly to `out_dir`.
   - Update `espforge-emit/src/project.rs::layer_overlay` to:
     - Replace `partitions.csv`, `rust-toolchain.toml`, `.cargo/config.toml`,
       `.cargo/esp-config.toml` verbatim if provided in `overlay`.
     - Pass `Cargo.extra.toml` (if present in `overlay`) into
       `codegen::cargo::merge(Some(extra_toml), &generated_cargo)` to merge
       user `[dependencies]`, `[patch]`, `[profile]`, and `[lints]` into the
       generated `Cargo.toml`.

2. **New module `espforge-emit/src/codegen/partitions.rs`**:
   Generates default `partitions.csv` from `Meta::ota.flash_size` (aligned to
   `0x10000` / 64 KiB):

   | `flash_size` | `ota_0` offset / size | `ota_1` offset / size |
   |---|---|---|
   | `"2MB"` | `0x10000` / `0xF0000` (960 KiB) | `0x100000` / `0xF0000` (960 KiB) |
   | `"4MB"` (default) | `0x10000` / `0x1F0000` (~1.94 MiB) | `0x200000` / `0x1F0000` (~1.94 MiB) |
   | `"8MB"` | `0x10000` / `0x3F0000` (~3.94 MiB) | `0x400000` / `0x3F0000` (~3.94 MiB) |
   | `"16MB"` | `0x10000` / `0x7F0000` (~7.94 MiB) | `0x800000` / `0x7F0000` (~7.94 MiB) |

   Layout for `4MB`:
   ```csv
   # Name,   Type, SubType,  Offset,   Size,     Flags
   nvs,      data, nvs,      0x9000,   0x4000,
   otadata,  data, ota,      0xd000,   0x2000,
   phy_init, data, phy,      0xf000,   0x1000,
   ota_0,    app,  ota_0,    0x10000,  0x1F0000,
   ota_1,    app,  ota_1,    0x200000, 0x1F0000,
   ```

3. **`espforge-emit/src/codegen/plan.rs`**:
   When `opts.is_on("ota")` is true, include `PlannedFile { out_path: "partitions.csv", source: Source::Partitions }`.

4. **`espforge-emit/src/codegen/config.rs`**:
   In `runner(opts, chip)` (and also if an overlay `partitions.csv` was provided):
   When OTA (or a custom `partitions.csv`) is active and `probe-rs` is off:
   ```text
   espflash flash --monitor --chip <chip> --partition-table ./partitions.csv --erase-parts otadata
   ```

5. **`espforge-emit/src/codegen/cargo.rs`**:
   When `opts.is_on("ota")` is true:
   - Include `"ota"` in `espforge-runtime`'s features list.
   - Add `esp-storage = { version = "0.10.0", features = [chip_name, "embedded-storage"] }`.
   - Add `esp-hal-ota = { version = "0.7.2", features = [chip_name, "log"] }`.

6. **`espforge-emit/src/codegen/rust.rs`**:
   When `meta.ota.is_some()`:
   - In `generated.rs`: emit
     `static __OTA_STORAGE: espforge_runtime::OtaStorage = espforge_runtime::OtaStorage::new();`
     (or expose a constructor helper `init_ota(registry.peripherals.FLASH)`).
   - In `lib.rs`: add `pub ota: espforge_runtime::Ota,` to `Context`.
   - In `main.rs`: initialize `ota` from `registry.peripherals.FLASH` before
     `Context { ... }` is packed.

---

### 2.6 `espforge-examples`

Add `espforge-examples/src/examples/05.Networking/ota_update/`:
- `example.espf.txt` (`<ota flash_size="4MB" rollback="true" />`, `<wifi>`, `<wifi_station>`, `<http_client>`)
- `example.toml` (`chips = ["esp32c3", "esp32s3"]`, `runtimes = ["embassy"]`)
- `app/rust/app.rs` (calls `ctx.ota.mark_app_valid()`, fetches manifest + streams image, calls `ctx.ota.finish()` and `ctx.ota.reboot()`)

---

## 3. Verification & Testing Plan

1. **Host Unit Tests (`cargo test --workspace`)**:
   - Schema parses `<ota />` with defaults and rejects invalid `flash_size`.
   - `LowerCx` holds `FLASH` when `<ota />` is present; `RESERVED_IDENTS` rejects `id="ota"`.
   - `FilePlan` omits `partitions.csv` when `ota` is off and includes it when on; existing non-OTA goldens remain unchanged.
   - `overlay` tests: custom `partitions.csv` and `Cargo.extra.toml` in source dir override/merge into generated `build/` cleanly.
   - `no_static_storage_in_runtime` still passes.
2. **Target Compile Gate**:
   Generate and build `05.Networking/ota_update` for `riscv32imc-unknown-none-elf` (and `xtensa-esp32s3-none-elf` where Xtensa toolchain is installed).
3. **Hardware End-to-End Test**:
   - Flash `v1` via `espforge build` (`espflash` with `--partition-table ./partitions.csv --erase-parts otadata`).
   - Build `v2` (`--release`), extract app image via `espflash save-image`, serve it with its size + CRC32.
   - Verify `v1` downloads `v2` to `ota_1`, resets into `v2`, and calls `mark_app_valid()`.
   - Verify **rollback**: flash a `v2` that omits `mark_app_valid()` and resets; confirm the bootloader reverts to `v1` on `ota_0`.
```

---

## Document 2: `ota-user-guide.md`

```markdown
# OTA User Guide (`espforge`)

This guide walks through setting up, building, flashing, and deploying
Over-The-Air (OTA) firmware updates in an `espforge` project powered by
[`esp-hal-ota`](https://github.com/filipton/esp-hal-ota).

---

## How OTA Works on ESP32

`espforge` configures a dual-slot A/B partition layout on your ESP32's flash:

```text
┌──────────┬──────────┬──────────┬────────────────────┬────────────────────┐
│   nvs    │ otadata  │ phy_init │       ota_0        │       ota_1        │
└──────────┴──────────┴──────────┴────────────────────┴────────────────────┘
                                    ▲ active (v1)        ▲ written by OTA (v2)
```

1. Your initial USB flash writes firmware **v1** into `ota_0`.
2. While running from `ota_0`, your firmware downloads **v2** in chunks and
   streams it into `ota_1`.
3. `esp-hal-ota` verifies the CRC32 checksum, reads back the flash to confirm
   integrity, marks `ota_1` as pending verification in `otadata`, and reboots.
4. On boot, **v2** runs its health checks (e.g., connecting to Wi-Fi) and calls
   `ctx.ota.mark_app_valid()`.
   - **If confirmed:** `ota_1` becomes permanent.
   - **If v2 crashes or resets before confirming:** the ESP-IDF bootloader
     automatically rolls back to **v1** in `ota_0`.

### Chip Support
`esp-hal-ota` is tested on **`esp32`**, **`esp32c3`**, and **`esp32s3`**.
(`esp32c2`, `esp32c5`, `esp32c6`, `esp32c61`, `esp32h2`, `esp32s2`, and
`esp32s31` have support in `esp-hal-ota` but are marked untested upstream.)

---

## Step 1: Enable `<ota />` in Your `.espf` File

Add `<ota />` inside the `# espforge` section of your `.espf` file:

```xml
====================
OTA DEMO
====================

# espforge
  <project name="ota_demo" />
  <chip type="esp32c3" />
  <runtime type="embassy" />
  <ota flash_size="4MB" rollback="true" />

# peripherals
  <wifi id="radio" />

# devices

# services
  <wifi_station id="home" wifi="$radio" ssid="YourNetwork" password="env:WIFI_PASSWORD" />
  <http_client id="http" network="$home" />
```

### `<ota />` Attributes
- **`flash_size`** (`"2MB"`, `"4MB"`, `"8MB"`, `"16MB"`, default `"4MB"`):
  Sizes the two app slots (`ota_0`, `ota_1`) in the generated `partitions.csv`.
  Make sure this matches the physical flash size on your board.
- **`rollback`** (`"true"` or `"false"`, default `"true"`):
  When `"true"`, newly booted OTA firmware **must** call
  `ctx.ota.mark_app_valid()`, or the bootloader will revert on the next reset.

> **Note:** `<ota />` reserves the `FLASH` peripheral exclusively so no other
> component can corrupt flash mid-update.

---

## Step 2: Update `src/app.rs`

There are two parts to every OTA-capable `app.rs`:
1. **Confirming the running firmware on boot** (`mark_app_valid()`).
2. **Downloading and flashing a new image** (`begin` → `write` → `finish` → `reboot`).

### Example (`src/app.rs`)

```rust
use crate::{service, Context};
use embassy_executor::Spawner;

pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    ctx.logger.info("OTA demo booting...");

    // 1. Bring up Wi-Fi (or run whatever self-check proves this firmware works)
    service!(ctx, home).wait_connected().await;
    ctx.logger.info("Wi-Fi connected");

    // 2. Mark this slot valid so the bootloader will not roll back on reset.
    if let Err(e) = ctx.ota.mark_app_valid() {
        ctx.logger.warn(format_args!("mark_app_valid failed: {e:?}"));
    }

    // 3. Example: fetch metadata (size + CRC32) then stream firmware.bin
    let image_size: u32 = 412_800; // From your update server / manifest
    let target_crc32: u32 = 0x1A2B_3C4D; // CRC32 of firmware.bin

    if let Err(e) = apply_ota(ctx, "http://192.168.1.50:8000/firmware.bin", image_size, target_crc32).await {
        ctx.logger.error(format_args!("OTA failed: {e:?}"));
    }
}

async fn apply_ota(
    ctx: &'static Context,
    url: &str,
    image_size: u32,
    target_crc32: u32,
) -> Result<(), &'static str> {
    ctx.ota.begin(image_size, target_crc32).map_err(|_| "ota begin failed")?;

    let mut done = false;
    service!(ctx, http)
        .get_stream(url, |chunk| {
            done = ctx.ota.write(chunk).map_err(|_| ())?;
            if let Some(pct) = ctx.ota.progress() {
                ctx.logger.info(format_args!("OTA progress: {pct}%"));
            }
            Ok::<(), ()>(())
        })
        .await
        .map_err(|_| "download/write failed")?;

    if !done {
        return Err("incomplete firmware download");
    }

    // Verifies CRC32 + reads back written flash, then updates otadata
    ctx.ota.finish().map_err(|_| "ota verify/finish failed")?;

    ctx.logger.info("OTA verified! Rebooting into new slot...");
    ctx.ota.reboot();
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(10_000).await;
}
```

> **Tip:** Never call `mark_app_valid()` at the very top of `setup()` before
> testing Wi-Fi or critical peripherals. Wait until your device has proved it can
> still connect and receive future updates—otherwise a broken Wi-Fi config could
> lock out future OTAs.

---

## Step 3: Customizing Files with Overlays (Optional)

`build/` is regenerated cleanly on every `espforge build`. Any file you want to
control directly belongs **next to your `.espf` file** as an overlay:

```text
my-ota-app/
├── my_ota_app.espf
├── src/app.rs
├── partitions.csv        # Optional: override the generated 2-slot partition table
├── Cargo.extra.toml      # Optional: add extra crates (e.g., serde_json, crc32fast)
└── .cargo/
    └── esp-config.toml   # Optional: custom [env] variables (ESP_LOG, SSID, etc.)
```

### Custom Partition Table (`partitions.csv`)
If you need extra partitions (like a larger `nvs`, `coredump`, or `littlefs` data
partition), create `partitions.csv` next to your `.espf`:

```csv
# Name,   Type, SubType,  Offset,   Size,     Flags
nvs,      data, nvs,      0x9000,   0x4000,
otadata,  data, ota,      0xd000,   0x2000,
phy_init, data, phy,      0xf000,   0x1000,
ota_0,    app,  ota_0,    0x10000,  0x1F0000,
ota_1,    app,  ota_1,    0x200000, 0x1F0000,
```

> **Rules for custom `partitions.csv`:**
> - Keep `otadata` (`data, ota`, size `0x2000`).
> - Keep both `ota_0` (`app, ota_0`) and `ota_1` (`app, ota_1`).
> - Align app partition offsets to `0x10000` (64 KiB).

---

## Step 4: Initial USB Flash (Factory Provisioning)

Build and flash your **v1** firmware over USB:

```sh
espforge build --release
cd build && cargo run --release
```

Because `<ota />` is enabled, `espforge` automatically configures `.cargo/config.toml`
to flash using:

```sh
espflash flash --monitor --chip esp32c3 --partition-table ./partitions.csv --erase-parts otadata
```

> **Why `--erase-parts otadata` matters:** If the board previously booted from
> `ota_1`, flashing a fresh image over USB writes to `ota_0` while `otadata`
> still points to `ota_1`. Erasing `otadata` forces the bootloader back to
> `ota_0`.

---

## Step 5: Building & Serving an OTA Update Binary (v2)

1. Make your code changes in `src/app.rs` and compile a **release** build:
   ```sh
   espforge build --release
   ```
   *(Release builds are strongly recommended so the binary fits comfortably inside
   your OTA partition and downloads quickly.)*

2. Convert the compiled ELF into a raw OTA application `.bin` using `espflash save-image`:
   ```sh
   espflash save-image --chip esp32c3 \
     build/target/riscv32imc-unknown-none-elf/release/ota_demo \
     firmware.bin
   ```
   > **CRITICAL:** Do **NOT** pass `--merge` to `espflash save-image`.
   > `--merge` prepends the bootloader and partition table for full-chip USB
   > flashing. An OTA payload must be the **application image only**.

3. Record the file size and CRC32 of `firmware.bin` for your OTA server:
   ```sh
   python3 -c "
   import zlib, pathlib
   data = pathlib.Path('firmware.bin').read_bytes()
   print('size:', len(data))
   print('crc32:', hex(zlib.crc32(data) & 0xffffffff))
   "
   ```

4. Serve `firmware.bin` from your update server (for local testing):
   ```sh
   python3 -m http.server 8000
   ```

---

## Step 6: Verifying Rollback Safety

Before deploying to real devices, test that automatic rollback works:
1. Flash **v1** (which logs `"Version 1"`) over USB.
2. Build **v2** (which logs `"Version 2"`), temporarily **omit**
   `ctx.ota.mark_app_valid()`, and call `ctx.ota.reboot()` (or trigger a panic/reset).
3. Perform the OTA update from v1 → v2.
4. Observe v2 boot once (`"Version 2"`), reset, and automatically boot back
   into **`"Version 1"`** on `ota_0`.

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| OTA succeeds, reboot still runs old firmware | `otadata` was never erased before USB flash, or `mark_app_valid()` wasn't called | Flash once via USB with `--erase-parts otadata`; verify `mark_app_valid()` runs on boot |
| `Ota::finish()` fails CRC / verification | Passed `--merge` to `espflash save-image`, or wrong `crc32` / `image_size` | Regenerate `firmware.bin` without `--merge` and recompute `len` + `zlib.crc32` |
| Image does not fit in partition | Debug binary too big or `flash_size` too small | Build with `--release` (`espforge build --release`) or increase `flash_size` / slot size in `partitions.csv` |
| `FLASH is held by OTA updates` error | Another element in `.espf` tried to claim `FLASH` | Remove the conflicting flash element or remove `<ota />` |
| Download drops mid-transfer over Wi-Fi | Long blocking flash operation or server timeout | Increase `timeout_ms` on `<http_client>` and stream chunks in 2–4 KiB pieces rather than buffering |
```
