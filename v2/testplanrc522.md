# RC522 Device Driver Test Plan — `embedded-hal-mock`

Target driver: `espforge-runtime/src/devices/rc522.rs` (`Rc522`)
Bindings driver: `espforge-bindings/src/devices/rc522.rs` (`Rc522Driver`)
Feature flag: `espforge-runtime` `rc522 = []` in `espforge-runtime/Cargo.toml:58`
Date: 2026-09-15

## 1. Goal

Host-side unit tests for the Phase-1 RC522 transport + reset + init, using
`embedded-hal-mock` (`eh1` API), runnable with `cargo test` on x86_64.

Phase-1 scope (from `rc522.rs:1-10`):

- SPI transport: `read_reg`, `write_reg`, `write_regs`, `read_regs`
- `version()`
- `soft_reset()`
- `init()`

Out of scope: tag detect / anti-collision / MIFARE read-write, async/embassy
variant, codegen/emitter tests, on-hardware HIL.

Done when:

- [ ] `cargo test -p espforge-runtime --features rc522` passes on host.
- [ ] Every public method in `rc522.rs:49-161` has at least one happy + one edge test.
- [ ] `init()` byte sequence matches ESPHome `initialize_` exactly.
- [ ] All mocks verified with `.done()` (no unconsumed expectations).

## 2. ADHD Operating Rules

1. One mission at a time. Stop after any checkbox block — progress saves.
2. 10–25 min timeboxes. If stuck >15 min, commit and ask.
3. Always end a session with a green `cargo test`.
4. Only file to edit for tests: `espforge-runtime/src/devices/rc522.rs` (`#[cfg(test)] mod tests` at bottom).
5. Only files to edit for refactor: same file + `espforge-runtime/Cargo.toml`.
6. No yak-shaving: no new crates, no HIL, no emitter changes in this plan.

## 3. Blocker: Why It Does Not Test Today

`Rc522` in `rc522.rs:43-47` currently owns concrete target-only types:

```rust
pub struct Rc522 {
    spi: SpiDevice<Blocking>,          // espforge-runtime/src/components/spi.rs:178
    reset: RefCell<Option<Output<'static>>>, // esp-hal gpio
    delay: Delay,                      // espforge-runtime/src/lib.rs:92, wraps esp-hal delay
}
```

`espforge-runtime/src/lib.rs:11` is `#![no_std]` + `esp-hal`.
`esp-hal` does not compile for host `cargo test`. Mocks cannot substitute
for concrete `SpiDevice<Blocking>` / `Output` / `Delay`.

Fix: Mission 1 makes `Rc522` generic over `embedded-hal` traits. No logic
change, production uses a type alias.

## 4. Prerequisites

- [ ] Rust host toolchain (`cargo test` works for `espforge` crate today).
- [ ] `embedded-hal = "*"` already in `espforge-runtime/Cargo.toml:12`.
- [ ] Add dev-dependency (Mission 0):

```toml
[dev-dependencies]
embedded-hal-mock = { version = "0.11", features = ["eh1"] }
```

Reference docs during work:

- `embedded-hal-mock` docs: `eh1::spi::Mock`, `eh1::spi::Transaction`,
  `eh1::digital::Mock`, `eh1::delay::NoopDelay`.
- MFRC522 datasheet §8.1.2.3 (SPI framing), §8.8.2 (reset timing).
- ESPHome `rc522_spi.cpp` `pcd_read_register`, `pcd_reset_`, `initialize_`.

## 5. Register Map Cheat Sheet (pre-shifted, `reg << 1`)

From `rc522.rs:21-34`:

| Name | `reg` | Read addr `0x80\|reg` | Notes |
|---|---|---|---|
| `COMMAND_REG` | `0x02` | `0x82` | bit 4 = PowerDown, val `PCD_SOFT_RESET = 0x0F` |
| `MODE_REG` | `0x22` | `0xA2` | init `0x3D` (CRC preset 0x6363) |
| `TX_MODE_REG` | `0x24` | `0xA4` | init `0x00` |
| `RX_MODE_REG` | `0x26` | `0xA6` | init `0x00` |
| `TX_ASK_REG` | `0x2A` | `0xAA` | init `0x40` (100% ASK) |
| `MOD_WIDTH_REG` | `0x48` | `0xC8` | init `0x26` |
| `T_MODE_REG` | `0x54` | `0xD4` | init `0x80` |
| `T_PRESCALER_REG` | `0x56` | `0xD6` | init `0xA9` |
| `T_RELOAD_H` | `0x58` | `0xD8` | init `0x03` |
| `T_RELOAD_L` | `0x5A` | `0xDA` | init `0xE8` |
| `VERSION_REG` | `0x6E` | `0xEE` | `version()` |

Reads: address byte `0x80 | reg`, then clock out (`read_reg`, `rc522.rs:60-64`).
Writes: address byte `reg`, then value, CS held (`write_reg`, `rc522.rs:67-69`).

## 6. Mission 0 — Deps (5 min)

- [ ] Add `embedded-hal-mock` to `[dev-dependencies]` as above.
- [ ] Verify: `cargo build -p espforge-runtime --features rc522` still passes.
- [ ] Stop point. Do not write tests yet.

## 7. Mission 1 — Genericize `Rc522` (20–30 min, required)

Goal: same behavior, testable on host.

### 7.1 Change

```rust
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiDevice as SpiDeviceTrait;

pub struct Rc522<SPI, RST, DLY> {
    spi: RefCell<SPI>,
    reset: RefCell<Option<RST>>,
    delay: RefCell<DLY>,
}

impl<SPI, RST, DLY, E> Rc522<SPI, RST, DLY>
where
    SPI: SpiDeviceTrait<u8, Error = E>,
    RST: OutputPin,
    DLY: DelayNs,
{
    pub fn new(spi: SPI, reset: Option<RST>, delay: DLY) -> Self { ... }

    // bodies unchanged except:
    // self.spi.borrow_mut().transfer_in_place(..).map_err(|_| SpiError::Bus)?
    // self.spi.borrow_mut().transaction(..).map_err(|_| SpiError::Bus)?
    // self.delay.borrow_mut().delay_ms(50);
    // self.delay.borrow_mut().delay_ns(2_000);
    // reset.set_low() / set_high() via OutputPin trait, map err to SpiError::Bus
}
```

Why `RefCell` + `borrow_mut()`: `embedded_hal::spi::SpiDevice::transaction`
takes `&mut self`, but `Rc522` API takes `&self` (see `rc522.rs:60,67,74,88`
and `components/spi.rs:242-267` interior-mutability pattern). Keep `&self`.

Error strategy: keep `Result<_, SpiError>` (`components/spi.rs:58-61`).
Map any mock/real SPI/pin error to `SpiError::Bus` with `.map_err(|_| SpiError::Bus)?`.
Avoids generic error param.

### 7.2 Production alias (no codegen break)

```rust
// in rc522.rs, gated for target builds:
pub type EspRc522 = Rc522<crate::components::spi::SpiDevice<esp_hal::Blocking>, esp_hal::gpio::Output<'static>, crate::Delay>;
```

- [ ] Update `espforge-bindings/src/devices/rc522.rs:62` ctor name if it names `Rc522` explicitly (still `Rc522`, alias resolves).
- [ ] Gate `esp_hal` imports with `#[cfg(not(test))]` or move alias to separate `#[cfg(not(test))]` block so host `cfg(test)` does not pull `esp_hal`.
- [ ] Verify target build still passes.
- [ ] Stop point. Do not write tests yet.

Alternative if alias proves messy: define minimal `Rc522Spi` trait with `&self`
`write / transfer_in_place / transaction`, impl for real `SpiDevice<Blocking>`
and for `RefCell<SpiMock>`. Prefer the generic `SpiDeviceTrait` approach first.

## 8. Mission 2 — First Green: `version()` (10 min)

Location: bottom of `rc522.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal_mock::eh1::{delay::NoopDelay, spi::{Mock as SpiMock, Transaction as SpiTx}};
}
```

Case V1 (happy):

- Given `VERSION_REG = 0x6E`, read bytes `[0xEE, 0x00]`.
- Mock: `SpiTx::transfer_in_place(vec![0xEE, 0x00], vec![0x00, 0x92])`.
- Build `Rc522::new(spi_mock, None::<NoopPin>, NoopDelay::new())`.
- Assert `version() == Ok(0x92)`, then `spi_mock.done()`.

Pin type for `None`: use `embedded_hal_mock::eh1::digital::Mock` as `RST`
param, pass `None`.

- [ ] V1 passes: `cargo test -p espforge-runtime --features rc522 version -- --nocapture`
- [ ] Stop point. Dopamine checkpoint.

## 9. Mission 3 — Transport Tests

### 9.1 `read_reg` (`rc522.rs:60-64`)

- [ ] R1 happy: reg `0x02` -> `transfer_in_place([0x82,0],[x,0xAB])`, assert `0xAB`.
- [ ] R2 error propagates: mock returns SPI error -> assert `Err(SpiError::Bus)`.
- [ ] Template for all later reads.

Mock pattern:

```rust
SpiTx::transfer_in_place(vec![0x80 | reg, 0x00], vec![0x00, value])
```

### 9.2 `write_reg` (`rc522.rs:67-69`)

Current impl uses `self.spi.write(&[reg, value])` which lowers to
`transaction([Write([reg,value])])`.

- [ ] W1 happy: `write_reg(0x24, 0x00)` -> `SpiTx::transaction(vec![0x24, 0x00])` (check exact mock variant name: `Transaction::write` vs `transaction`; adapt to 0.11 API).
- [ ] W2 error -> `Err`.

### 9.3 `write_regs` FIFO (`rc522.rs:73-78`)

Impl: `transaction([Write([reg]), Write(values)])`, CS held.

- [ ] WF1 multi-byte: `write_regs(0x09<<1, &[1,2,3])` -> two `Write` ops in one transaction.
- [ ] WF2 empty slice: still one transaction with empty second write (document behavior, do not change impl to special-case).
- [ ] WF3 error -> `Err`.

### 9.4 `read_regs` FIFO (`rc522.rs:88-113`)

Impl quirks to pin (from ESPHome port):

- `rx[0]` junk, `values[i] = rx[i+1]`.
- Address `0x80|reg` re-sent for every byte except last; last sends `0x00`.
- Single `transfer_in_place` of `len+1` bytes, CS held.
- `values.len() > 64` -> `Err(Bus)`.
- `rx_align 0..=7` merges bits into `values[0]`.

Cases:

- [ ] RF1 empty -> `Ok(())`, no SPI traffic (`mock.done()` with zero expectations).
- [ ] RF2 len 1: `read_regs(reg, &mut [0], 0)` sends `[addr, 0x00]`, returns payload byte.
- [ ] RF3 len 3, `rx_align=0`: sends `[addr,addr,addr,0x00]`, copies `buf[1..]`.
- [ ] RF4 `rx_align=4`: same traffic, `values[0] = (values[0] & !mask) | (buf[1] & mask)` where `mask = 0xFF << 4 = 0xF0`. Use distinct nibbles to prove merge.
- [ ] RF5 too long (65 bytes) -> `Err(SpiError::Bus)`, no SPI traffic.
- [ ] RF6 underlying SPI error -> `Err`.

## 10. Mission 4 — `soft_reset` (`rc522.rs:123-133`)

Sequence: `write_reg(COMMAND 0x02, 0x0F)`, `delay_ms(50)`, loop 3x
`read_reg(COMMAND)` check bit 4 clear.

- [ ] S1 immediate clear: write + 1 read returning `0x00` -> `Ok`.
- [ ] S2 delayed clear: reads `0x10, 0x10, 0x00` -> `Ok`, verify 3 reads consumed.
- [ ] S3 stuck: reads `0x10, 0x10, 0x10` -> `Err(Bus)`.
- [ ] S4 write fails -> `Err`, no reads.
- [ ] Use `NoopDelay` — do not assert timing, only call order.

## 11. Mission 5 — `init` (`rc522.rs:138-161`)

Exact ESPHome `initialize_` order:

```
soft_reset traffic (see Mission 4)
0x24,0x00 (TX_MODE)
0x26,0x00 (RX_MODE)
0x48,0x26 (MOD_WIDTH)
0x54,0x80 (T_MODE)
0x56,0xA9 (T_PRESCALER)
0x58,0x03 (T_RELOAD_H)
0x5A,0xE8 (T_RELOAD_L)
0x2A,0x40 (TX_ASK)
0x22,0x3D (MODE)
```

Plus optional hard reset when `Some(rst)`:

```
set_low, delay_ns(2_000), set_high, delay_ms(50)
```

- [ ] I1 `init` with `reset=None`, soft-reset clears first try -> full write list in order, `Ok`.
- [ ] I2 `init` with `Some(mock_pin)`: pin expectations `[Set(Low), Set(High)]` + same SPI list, `Ok`.
- [ ] I3 `soft_reset` stuck inside `init` -> `Err`, no further writes (mock verifies truncation via `.done()` failure if extra expectations remain).
- [ ] I4 mid-`init` write fails (e.g. 4th write errors) -> `Err`.

Pin mock: `eh1::digital::Mock` with `Transaction::set(State::Low/High)` (check
0.11 naming: `digital::Transaction::set`). Delay mock: `NoopDelay`.

## 12. Mission 6 — Hardening (optional, after green)

- [ ] Error mapping: pin `set_low` failure -> `Err(Bus)`.
- [ ] `read_regs` `rx_align` 1 and 7 boundary.
- [ ] `write_regs` large (64B) FIFO boundary.
- [ ] Clippy: `cargo clippy -p espforge-runtime --features rc522 --tests`.
- [ ] Fmt: `cargo fmt --check`.

## 13. Commands

```sh
# target build (per AGENTS.md)
cargo build -p espforge

# host unit tests for rc522
cargo test -p espforge-runtime --features rc522 -- --nocapture
cargo test -p espforge-runtime --features rc522 rc522 -- --nocapture

# single test
cargo test -p espforge-runtime --features rc522 version -- --nocapture

# lint
cargo clippy -p espforge-runtime --features rc522 --tests
```

If host build still pulls `esp-hal`, the Mission 1 `#[cfg]` split is
incomplete — fix imports before adding more tests.

## 14. Risks / Notes

1. `embedded-hal-mock` 0.11 API names (`Transaction::transfer_in_place`,
   `write`, `transaction`) differ slightly from 0.10 — check docs at write
   time, keep one helper `fn spi_mock(expectations) -> SpiMock`.
2. `SpiDevice::write` vs `transaction([Write])` lowering: assert what the
   mock actually sees; adjust expectation, not driver.
3. `DelayNs::delay_ms` default impl calls `delay_ns` — `NoopDelay` is fine.
4. Keep `&self` driver API (ADR-008 shared `&` context). Do not change public
   call sites to `&mut`.
5. Do not test codegen in this plan (`espforge-bindings/.../rc522.rs`
   `construct()` is covered by existing emitter tests).

## 15. Checklist Summary (copy to PR)

- [ ] Mission 0 deps
- [ ] Mission 1 generic `Rc522<SPI,RST,DLY>` + `EspRc522` alias, target builds
- [ ] Mission 2 `version` green
- [ ] Mission 3 `read_reg` / `write_reg` / `write_regs` / `read_regs`
- [ ] Mission 4 `soft_reset` 4 cases
- [ ] Mission 5 `init` 4 cases (+ pin mock)
- [ ] `mock.done()` in every test, clippy + fmt clean
