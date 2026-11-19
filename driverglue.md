# `driverglue.md`: The Crates a Manifest Cannot Name

**Baseline:** `espforge` workspace `0.9.1`, branch `dev`. `driverfixes.md` is
landed: generic `endpoint`, fixed constructor arguments, `fallible #false`,
per-mode dependency features and `E0911` all exist. `sht31`, `sht4x`, `ads111x`,
`bme680` and `mhz19b` ship as manifest-only drivers, and `ssd1306`, `ili9341`,
`scd4x`, `adxl345` as bundled ones.

**Purpose:** make the third class of driver usable from `.espf` — the crate whose
constructor is not one call and cannot become one. Each gets ~15 lines of Rust in
`espforge-drivers` behind its own feature, and an ordinary bundled manifest. KDL
does not change.

**Explicitly not in this document:** PWM/ADC/1-Wire/RMT buses, `provides`
(expanders, muxes), dynamic `<logger>` targets, nested `compose`, and glue for
*user* driver packages — those can already ship their own Rust crate through
`dependency { path ".." }` or `dependency { git ".." rev ".." }`.

**Appendix A** is the forward-looking half: what an additive `codegen` extension
would have to look like to move this same set from Rust glue into the manifest.

---

## 1. What Glue Is, and the Three Kinds of Driver

A manifest's `constructor "..."` emits exactly one call with positional
arguments:

```rust
air: embedded_sht3x::Sht3x::new(bus_device, 0x44, espforge_runtime::Delay::new())
    .map_err(|_| espforge_runtime::DeviceError { device: "air", kind: DeviceErrorKind::Init })?,
```

Three driver shapes exist against that rule:

| Kind | Constructor shape | Where the Rust lives | Example today |
|---|---|---|---|
| **Manifest only** | one call on a path | nowhere — the vendor crate | `sht31`, `sht4x`, `ads111x`, `bme680`, `mhz19b` |
| **Bundled driver** | one call on a path | `espforge-drivers/src/<tag>.rs`, implementing the protocol | `scd4x`, `ssd1306` |
| **Glue** | anything the vendor crate requires | `espforge-drivers/src/<tag>.rs`, ~15 lines composing upstream objects | this document |

Glue is the smallest possible Rust: it maps the bus handle espforge already
hands out onto whatever the crate's API demands — a second object, a builder
chain, a struct literal, a typestate, or a post-construction `init()` — and then
gets out of the way. It contains no protocol logic. If a module needs CRC, register
maps or state machines, it is a bundled driver, not glue.

The decision to do it this way is recorded in `driverfixes.md` Milestone 5 and
`architecture/journey-driver-manifest-v2.md` §6: extending `constructor` to
arbitrary expressions would make the manifest a second programming language.

## 2. The Glue Contract

Every glue crate follows the same shape; this is what makes them maintenance
rather than design work.

### 2.1 Module and feature

```
espforge-drivers/src/<tag>.rs              # the glue
espforge-drivers/manifests/<tag>.kdl       # the manifest
espforge-drivers/Cargo.toml                # the feature
espforge-drivers/src/lib.rs                # MANIFESTS, DRIVERS_SCHEMA_KDL, TAGS, module gate
```

The feature is per crate, like the existing ones:

```toml
# The SPI e-paper display. Blocking and async init live in one function pair.
epd-waveshare = ["dep:epd-waveshare"]
epd-waveshare-async = ["epd-waveshare", "espforge-runtime/embassy"]
```

The module stays behind the target gate every bundled driver uses, so a host
`cargo test` never compiles `esp-hal`:

```rust
#[cfg(all(any(target_arch = "riscv32", target_arch = "xtensa"), feature = "epd-waveshare"))]
pub mod epd_waveshare;
```

### 2.2 Endpoint alias

The manifest's `endpoint` has to name the field of the deferred factory, so the
glue module exports the type the bus helper returns — the same pattern as
`Scd4xI2cEndpoint`, `Adxl345Endpoint` and `SpiEndpoint` in `ili9341`:

```rust
pub type Hd44780I2cEndpoint = CriticalSectionDevice<'static, I2c<'static, Blocking>>;
pub type AsyncHd44780I2cEndpoint = espforge_runtime::AsyncI2cBus;
```

### 2.3 One function per crate

```rust
/// Build the display from the shared I2C endpoint and the project's delay.
///
/// The crate wants an expander object and a delay, so this is the one place
/// that composition happens.
pub fn hd44780(
    i2c: Hd44780I2cEndpoint,
    address: u8,
    delay: Delay,
) -> Result<Hd44780<I2CExpander<Hd44780I2cEndpoint>>, DeviceErrorKind> {
    let expander = I2CExpander::new(i2c, address);
    Hd44780::new(expander, delay).map_err(|_| DeviceErrorKind::Init)
}
```

Rules, all forced by existing checks:

1. **It returns `Result<_, DeviceErrorKind>`.** `codegen { fallible #false }` is
   refused when `feature` is present (`espforge-schema/src/driver.rs`, the
   `build_recipe` check), because a bundled driver's error is the
   `espforge-drivers` API and lowering maps it to a named device error. So glue
   normalises the crate's error into `Init` or `Bus`, exactly as `scd4x` does.
2. **One function, not one wrapper struct.** A wrapper is allowed only when the
   crate's own type cannot be named in `type "..."` (§4). A wrapper never
   re-implements the crate's reads; it forwards to them.
3. **No `esp-hal` in the manifest.** The manifest names `espforge_drivers::…`
   only; all chip types stay inside the module.
4. **The async half is a separate function**, and may be `async fn`: the schema
   already supports `constructor` calls that are awaited (`awaits` defaults to
   `#true`).
5. **Blocking first.** A crate that only offers an async initializer still gets a
   blocking wrapper only if espforge can drive it; otherwise it is async-only and
   the manifest declares just `runtime "async"` with `awaits #true`.

### 2.4 Manifest

A glue driver is a bundled driver, so it uses `codegen.feature`, not
`dependency` for the crate it wraps (the vendor crate is still named as a
`dependency` so `Cargo.toml` gets it):

```kdl
driver "hd44780" {
    format_version 2
    summary "HD44780 character LCD behind a PCF8574 I2C expander"
    category "display"

    dependency "hd44780-embedded-hal" { version "<verify>" }

    bus "i2c" {
        address default="0x27"
    }

    codegen {
        feature "hd44780"
        runtime "blocking" {
            type "espforge_drivers::Hd44780Display"
            constructor "espforge_drivers::hd44780({bus_device}, {address}, {delay})"
            endpoint "espforge_drivers::Hd44780I2cEndpoint"
        }
        runtime "async" {
            feature "hd44780-async"
            type "espforge_drivers::AsyncHd44780Display"
            constructor "espforge_drivers::hd44780_async({bus_device}, {address}, {delay})"
            endpoint "espforge_drivers::AsyncHd44780I2cEndpoint"
        }
        map "delay" {
            "runtime" { expr "espforge_runtime::Delay::new()" }
        }
    }

    attr "delay" kind="choice" default="runtime" { choices "runtime" }
    example "minimal" #"<hd44780 id="lcd" bus="$i2c0" address="0x27" />"#
}
```

`type "espforge_drivers::Hd44780Display"` is the alias or newtype the glue
module exports; the vendor type may be generic and unwieldy, which is exactly
why the module names it once.

### 2.5 Proof

Text assertions do not prove that a constructor composition works. Each crate
gets one `Case` in `espforge/tests/generated_projects_compile.rs`, built from
`01.Basics/helloworld`, on `esp32c3` blocking (and an embassy twin when the
driver has an async half):

```rust
// The glued constructor: the crate wants an expander plus a delay, so the
// manifest names `espforge_drivers::hd44780` and the generated call composes
// both inside it.
Case {
    example: "01.Basics/helloworld",
    chip: "esp32c3",
    runtime: Runtime::Blocking,
    inline: Some("# espforge\n <project name=\"glue_hd44780\" />\n ..."),
},
```

---

## 3. The Queue

Ordered by cost. Signatures are sketches: verify every type parameter,
constructor name and feature against docs.rs before writing the manifest — the
same rule `driverfixes.md` set for the manifest-only drivers.

| # | Crate | Why one call cannot say it | Glue shape | Effort |
|---|---|---|---|---|
| 1 | `hd44780-embedded-hal` | display + PCF8574 expander + delay: three objects, one constructor | `hd44780(i2c, address, delay)` | S |
| 2 | `icm20948-async` | constructor returns a typestate; an initialisation call follows it | `icm20948(bus, cs, delay)` runs the steps | M |
| 3 | `epd-waveshare-async` | builder chain, then `init()` awaited | `epd_waveshare(bus, pins, delay)` (`async fn`) | M–L |
| 4 | `epdsi` | same builder + init shape over a second panel family | shares #3's shape; one function, not one per model | M |
| 5 | `lora-phy` | radio + modem + params + delay; generics bubble into the field type | `lora(bus, pins, delay)` returning a named type | L |
| 6 | `stts22h-rs` | typestate bus wrapper is not public | blocked on §4, not on code | ? |
| 7 | `bmi323-driver` | same typestate problem, plus per-mode features (already expressible) | blocked on §4 | ? |

Deliberately **not** in this queue: `ws2812-rs`. Its struct literal is only half
the problem — a manifest on `bus "spi"` always declares `cs` as required
(`add_bus_attrs`), and an LED strip has no chip select. Glue cannot manufacture a
cs-less bus handle from `{bus_device}`, so the crate needs the schema work in
Appendix A.4 before any Rust is worth writing.

---

## 4. The Type-Naming Problem (`stts22h-rs`, `bmi323-driver`)

These two fail differently from the others. Their API is
`Stts22h<I2cBus<P>, OnState>`, where `I2cBus` and the typestate's bound are not
public. Glue can *build* the value, but the manifest's `type "..."` must name the
field, and `endpoint` must name what the factory stores. A function cannot fix an
unnameable type.

Options, in order of preference:

1. **Upstream a public alias.** One `pub type Stts22hI2c<Bus> = Stts22h<I2cBus<Bus>, …>`
   in the crate (or a public wrapper type) turns this into the ordinary glue case
   and helps every user of the crate. This is the outcome to aim for.
2. **Pin a fork that exports it.** The schema already supports
   `dependency "<crate>" { git "https://…" rev "<sha>" }`, so a small
   `espforge-*` fork carrying only the alias is expressible today without any
   KDL change. The cost is supply chain: a fork needs an owner and a rebase
   story, and `driver verify` pins its manifest and payload but not its intent.
3. **A newtype in `espforge-drivers`.** Only works if some public type can name
   the inner value. If it cannot, the newtype cannot have a field of it either;
   the remaining escapes are `dyn` behind `alloc` (not available) or
   re-implementing the protocol (not glue). Do not take this path.
4. **Leave them out of this wave.** They are already recorded as out of KDL in
   `architecture/journey-driver-manifest-v2.md` §6 and in
   `research/07-driver-external-limitations.md`. Nothing is lost by waiting for
   option 1.

Decision rule: do not ship a wrapper that duplicates protocol logic to work
around a visibility problem. Either the crate exposes the alias, or the driver
waits.

---

## 5. Phases

### Phase 5.1: Contract and First Crate — `hd44780`
**Why:** the archetype. I2C only, so the `cs` question does not arise; three
objects composed in one place; both modes exist upstream.

**Files to modify**
- `espforge-drivers/Cargo.toml` (`hd44780`, `hd44780-async` features)
- `espforge-drivers/src/hd44780.rs` (new)
- `espforge-drivers/src/lib.rs` (`MANIFESTS`, `DRIVERS_SCHEMA_KDL`, `TAGS`, module gate)
- `espforge-drivers/manifests/hd44780.kdl` (new)
- `espforge/tests/generated_projects_compile.rs` (two cases)
- `espforge-elements/src/schema.rs` (the bundled-shape test)

**Steps**
1. Verify on docs.rs: `hd44780-embedded-hal`'s `Hd44780::new` signature, whether
   it takes the delay by value, and the async variant's feature name. Write the
   manifest against what exists, not against this sketch.
2. Write the module: endpoint aliases, `hd44780`, `hd44780_async`, and a
   `DeviceErrorKind::Init` mapping. Export the concrete `Hd44780Display` alias
   (`type` text in the manifest cannot be generic over the expander).
3. Register the manifest in the three lists in `lib.rs` and add the target-gated
   module, exactly as `scd4x` is registered.
4. Extend the bundled-driver shape coverage in
   `espforge-elements/src/schema.rs` so the new tag's recipe loads and its
   constructor keeps all three arguments in order.
5. Add the blocking and embassy `Case`s.

**Verify**
```sh
cargo test -p espforge-schema -p espforge-elements
cargo test -p espforge --test golden_projects -- --ignored   # no churn expected
cargo test -p espforge --test generated_projects_compile -- --ignored
```

### Phase 5.2: A Typestate With a Public Alias — `icm20948`
**Steps**
1. Confirm what the crate exposes after construction (`new` → `initialize` →
   a usable type). The glue function performs the steps in one function and
   returns the post-init type; the manifest's `type` names the module's alias for
   it.
2. Decide whether the initialisation call needs the delay: if so, reuse the
   `map "delay"` trick rather than inventing an attribute.
3. Same registration, schema-shape test and compile case as Phase 5.1.

**Verify:** as Phase 5.1. If the init sequence needs a delay longer than the
`Delay` handle supports at boot, stop and report it rather than blocking the
boot path.

### Phase 5.3: Async Init — `epd-waveshare-async`, then `epdsi`
**Why:** the builder plus awaited `init()` is the first shape where the
constructor itself must be `async`. The schema supports it (`awaits` defaults to
`#true`), so this phase proves the async glue path.

**Steps**
1. Write `async fn epd_waveshare(...) -> Result<EpdType, DeviceErrorKind>` that
   runs the builder chain, then `init(&mut delay).await`, mapping both failures.
2. Manifest: one `runtime "async"` block, `feature "epd-waveshare-async"`,
   `endpoint "espforge_drivers::AsyncEpdSpiEndpoint"`. No blocking runtime — do not
   claim a blocking mode the crate does not offer.
3. Add the pins the panel needs as `pin` declarations (`dc`, `reset`, `busy`) so
   lowering claims them; no `PinOrRef` on an auxiliary pin in this wave.
4. `epdsi` follows only if it reduces to the same function shape; otherwise it
   gets its own note in this document and waits.

**Verify**
```sh
cargo test -p espforge --test generated_projects_compile -- --ignored
```
The embassy case is the only place an awaited constructor is compiled.

### Phase 5.4: Multi-Object Radio — `lora-phy`
**Why:** the largest glue surface: modem, radio kind, params and delay, with
generics that must be pinned into one nameable field type.

**Steps**
1. Fix the concrete generic parameters in the module and export one alias; the
   manifest names the alias, never the raw generics.
2. Expose the operations app code needs by forwarding to the crate (`send`,
   `recv`, `set_frequency`), not by re-implementing the state machine.
3. One compile case, blocking. Async only if the crate's API is actually async
   behind the same feature set.

**Verify:** as Phase 5.1.

### Phase 5.5: Typestates — decision, then whatever it unlocks
**Steps**
1. Ask for the upstream alias (§4 option 1). If it lands, treat `stts22h` and
   `bmi323` as Phase 5.1 crates.
2. If it will not land in this wave, take option 4: no manifest, no module, and
   one line in `research/07-driver-external-limitations.md` recording that the
   blocker is visibility, not expressiveness.
3. Do not add a fork without an owner for rebases.

### Phase 5.6: Documentation and Gate
1. Add a "glue driver" note to `docs/002-external-driver-manifest.md`: a bundled
   driver whose constructor is a function in `espforge-drivers`, with the
   `DeviceErrorKind` rule and one worked example.
2. Update the "What stays out of KDL" section of
   `architecture/journey-driver-manifest-v2.md` §6 to point at the crates that are
   now covered and at Appendix A for the forward path.
3. Final gate:
```sh
cargo test --workspace
cargo test -p espforge --test golden_projects -- --ignored
cargo test -p espforge --test generated_projects_compile -- --ignored
cargo test -p espforge --test compile_fail -- --ignored
git diff --exit-code
```
Expected golden churn: **none**. No example uses the new tags; a diff means the
new manifests leaked into an existing project.

## 6. Acceptance Criteria

- `<hd44780 id="lcd" bus="$i2c0" address="0x27" />` compiles on `esp32c3`
  blocking, and the generated project names `espforge-drivers` with the
  `hd44780` feature and `hd44780-embedded-hal` as a dependency.
- `epd-waveshare`'s deferred/async factory compiles with an awaited constructor,
  and its `Devices::new` contains no builder chain.
- App code imports only `espforge_drivers::…` types; no `esp-hal` or vendor type
  appears in the manifest or in a `.espf`.
- Every glue module is behind a target gate and its own feature; two glued
  drivers in one project do not enable each other's crates.
- No glue module implements a protocol; each is a constructor composition of at
  most ~20 lines of body, plus imports and doc comments.
- `cargo test --workspace` is green and the goldens are byte-identical.

---

## Appendix A: Toward Manifest-Only

"Manifest-only" has two meanings, and they are usually conflated:

- **For the end user** — they write only `.espf`. Glue already delivers this: the
  user cannot tell `<hd44780>` from `<sht31>`.
- **For the espforge repo** — the per-crate Rust disappears from
  `espforge-drivers` entirely, and a third party could publish the driver as a
  KDL file alone.

The appendix is about the second. Each proposal is additive to
`format_version 2`: absent keys keep today's behaviour, so no new format version
and no fixture migration.

### A.1 The Expressiveness Ladder

A manifest currently sits at rung 1. Each rung up costs schema, emission,
diagnostics and tests — the jump gets more expensive faster than it gets more
useful.

| Rung | What `codegen` can say | Unlocks | Cost |
|---|---|---|---|
| 1 | one call on a path, positional args (today) | `sht31`, `sht4x`, `ads111x`, `bme680` | — |
| 2 | one *expression*: struct literals, `&cfg`, integers | `ws2812`'s literal (once A.4 lands) | low |
| 3 | a constructor followed by statement steps | `epd-waveshare`, `epdsi`, `icm20948` | medium–high |
| 4 | a builder chain as a step sequence with inferred receivers | `mipidsi`-style panels | high, low value |
| 5 | control flow (loops, conditionals, anonymous types) | almost nothing new | refuse |

The line to hold: **the manifest may name values and the order of calls; it may
not compute.** Rung 5 breaks the `syn` check that makes a manifest error a
manifest error.

### A.2 K1 — Literal Constructor Shape

*Rung 2. Unblocks `ws2812-rs` together with A.4.*

```kdl
codegen {
    feature "ws2812"
    runtime "blocking" {
        literal "espforge_drivers::WS2812SPI { led: {bus_device} }"
        type "espforge_drivers::WS2812Strip"
        endpoint "espforge_drivers::SpiEndpoint"
    }
}
```

Why it is cheap: the emitter already writes `#new(#handle, …)` as text; a
`literal` variant is the same substitution with the whole expression parsed by
`syn::Expr` after holes become sentinel identifiers. Placeholders are allowed
only in value position — `W { led: {bus_device}, count: {size} }` — never in the
type path or a field name.

Checks to add in `espforge-schema/src/driver.rs`, mirroring `parse_constructor`:

- parse as `syn::Expr`, refuse a statement, a block or a closure;
- refuse a `{placeholder}` in field-name or type position with a named error;
- refuse `literal` together with `constructor` (one shape per runtime);
- keep `fallible` working: a literal construction is usually infallible, but a
  `try_into` inside the expression may not be, so the flag stays meaningful.

Emission is a one-line change in `espforge-elements/src/model.rs`, because the
`new` text is already treated as opaque Rust after validation. Deferred factories
benefit unchanged: the body is `Ok(<expression>)`.

### A.3 K2 — Post-Construction Steps

*Rung 3. The highest-value rung: it covers e-paper, IMUs with init sequences, and
any crate that needs `init()` after `new()`.*

```kdl
runtime "async" {
    constructor "epd_waveshare::Epd::new({bus_device}, {dc}, {reset})"
    then { call "init({delay})" awaits #true }
    then { call "set_rotation({rotation})" }
    type "epd_waveshare::Epd<...>"
    endpoint "espforge_runtime::ConfiguredAsyncSpiDevice"
}
```

Semantics to specify, because they are where this design either stays honest or
becomes a macro language:

- **Order is the manifest's.** Steps run after construction, in written order.
- **A failing step fails the device**, mapped to `DeviceErrorKind::Init`; the
  constructed value is dropped, so a half-initialised device never reaches app
  code. That is the same guarantee `Devices::new` gives today.
- **Steps take no `&mut` receiver text.** The generated code calls the method on
  the just-constructed local; allowing an arbitrary receiver expression is how
  rung 5 sneaks in.
- **`awaits` is per step**, defaulting to the runtime's convention, so a sync
  `init` inside an async constructor stays expressible.
- **Deferred factories run the same steps** inside `init()`, not at boot, which
  keeps the existing deferred contract.

Cost: emission grows a statement list in three places (`Devices::new`,
deferred factory, async factory), and diagnostics need a span per step. That is
the bulk of the work; the schema side is a small `Vec<Step>`.

This proposal alone would obviate most of Phases 5.2–5.4 for drivers that only
need a method call, and would shrink the glue queue to the type-naming cases.

### A.4 K3 — Optional Chip Select

*Prerequisite for `ws2812-rs`; independent of the constructor shape.*

`<spi>`'s `cs` is optional at the bus level, but a `bus "spi"` manifest always
declares a required `cs` attribute (`add_bus_attrs`). Add a way to say "this
device does not use a select":

```kdl
bus "spi" {
    cs_via "none"
}
```

This is not a cosmetic schema change: `{bus_device}` currently means
`ConfiguredSpiDevice` (bus + select), and a cs-less device needs a different
handle — the bare `SpiBus` — so lowering and emission must learn a second SPI
transport for device elements, and `endpoint` must accept the matching type.
That is why `driverfixes.md` Milestone 5 deferred it to the `provides`/multi-peer
work. Do it when the bus layer grows multi-peer support, not before.

### A.5 K4 — Builder Chains

*Rung 4. Recommend against.*

A builder is a step sequence with a receiver convention:

```kdl
chain "mipidsi::Builder::new()" {
    step "display_size({width}, {height})"
    step "orientation({rotation})"
    step "build({bus_device})"
}
```

Tempting for `mipidsi`, but it buys little over K2: the common builder is a
sequence of `self -> Self` calls ending in one fallible `build`, which K2 plus a
`constructor` that names the builder's start can already express. Making the
manifest understand receivers, fallibility per step and generic inference is a
large amount of machinery for a handful of crates. Prefer K2; revisit only if
two or more crates prove K2 cannot reach them.

### A.6 K5 — Nested Composition

Panels that are a display plus a touch controller already use `compose`, but
only one level deep (a composed part cannot itself compose). Allowing recursion
needs cycle detection and a depth cap in lowering, plus a decision about whether a
nested part's pins are claimable from the parent. This is orthogonal to
constructors and probably belongs with the Wave 3 element work rather than here.

### A.7 What No Template Can Fix

- **Private typestate bus wrappers** (`stts22h-rs`, `bmi323-driver`): `type` must
  name the field. A template can only produce text; it cannot make a private type
  nameable. Upstream alias or a pinned fork, per §4.
- **Constructors that borrow the bus for less than `'static`**: a deferred
  factory stores the endpoint until `init()`, so a constructor taking
  `&mut Bus<'a>` with a caller-scoped lifetime cannot be deferred regardless of
  how it is spelled.
- **Initialisation that needs runtime data** (calibration coefficients read at
  boot, a value computed from another device): the manifest has no expression
  language, and giving it one is rung 5.

### A.8 Ordering and Decision Rule

1. **K1 (literal)** — smallest change, unlocks the literal case, and is a
   prerequisite for nothing else.
2. **K2 (steps)** — largest payoff per unit of schema work; subsumes most of the
   glue queue.
3. **A.4 (optional cs)** — do it with multi-peer SPI, then `ws2812` becomes K1 +
   A.4 with no Rust at all.
4. **K5 (nested compose)** — with the Wave 3 element work.
5. **K4 (builders)** — only if K2 is demonstrably insufficient.

Each proposal keeps the three invariants that make the current manifest
trustworthy: every template is `syn`-parsed at load, an unknown `{placeholder}`
is a manifest error, and absent keys mean today's behaviour. A proposal that
needs any of those weakened is out of scope for this document.

### A.9 Verification Shape

Each K-proposal lands with the same proof shape as the phases above: a
schema test that the shape parses and refuses the near-miss, a lowering test that
the emitted text is in the written order, a `generated_projects_compile` case
that actually builds it, and a golden check with **no** churn.

```sh
cargo test -p espforge-schema -p espforge-elements
cargo test -p espforge --test golden_projects -- --ignored
cargo test -p espforge --test generated_projects_compile -- --ignored
```
