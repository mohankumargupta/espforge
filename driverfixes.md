# `driverfixes.md`: Closing the Gaps That Keep Drivers Out of Pure KDL

**Baseline:** `espforge` workspace `0.9.0`, branch `dev`, with `drivers.md`
Milestones 1–3 landed and `sht31` shipped as the first manifest-only driver
(`espforge-drivers/manifests/sht31.kdl`, no module and no Cargo feature in
`espforge-drivers`).

**Purpose:** make the manifest-only path (`format_version 2` `driver` element,
no `codegen.feature`, `dependency` blocks) able to express the real
constructors of the crates `drivergap1.md` surveyed. Three capabilities are
missing today, one is half-built, and two of the survey's assumptions are
wrong.

**Explicitly not in this document:** PWM/ADC/RMT/I2S/OneWire buses, device
`provides` (expanders, muxes), dynamic `<logger>` targets, `PinOrRef` on
auxiliary pins. Those are Wave 3 items in `drivers.md` and are untouched here.

---

## Where the Survey Stands Against the Code

`drivergap1.md` claims 12 of 20 crates are "pure KDL" today. Measured against
the shipped schema, that number is too high, and one of its blockers is not
real:

| # | Claim in `drivergap1.md` | What the code actually does | Verdict |
|---|---|---|---|
| 1 | `fallible #false` is a `drivers.md` feature | No such key exists anywhere in `espforge-schema` or any manifest. `espforge-elements/src/model.rs:1403-1424` always appends `.map_err(..)?`, and the deferred factory always appends `.map_err(..)` | **Gap — Milestone 2.** Every `sht4x`/`axp2101-embedded` row is blocked, not just "pure KDL" |
| 2 | Constructor args may be literals/`Type::default()` | `parse_constructor` (`espforge-schema/src/driver.rs:1520`) accepts only `Expr::Path` sentinels; everything else is "a constructor argument must be a `{placeholder}`" | **Gap — Milestone 3.** `embedded-ads111x`, `bosch-bme680` |
| 3 | Deferred init works for a manifest driver | `endpoint` is validated with `is_rust_path` (`espforge-schema/src/driver.rs:1106-1118`), and `espforge-schema/src/lib.rs:1633` refuses generics. Every real bus endpoint is generic, so `init="deferred"` is unreachable without a bundled alias. This is why `sht31` ships boot-only | **Gap — Milestone 1** |
| 4 | Per-runtime features (`blocking_features`/`async_features`) needed by `stts22h-rs`, `bmi323-driver` | `Dependency` (`espforge-schema/src/lib.rs:199`) has one flat `features` list; `espforge-emit/src/codegen/cargo.rs:420-436` writes it once, for both runtimes; manifest-only packages hard-wire `default_features: true` (`Dependency::from_package`, `espforge-schema/src/lib.rs:225-231`) | **Gap — Milestone 4** |
| 5 | `ws2812-rs` is blocked by "SPI `cs` is currently `required=#true`" | `<spi>`'s `cs` is optional (`espforge-schema/src/espf.schema.kdl:180`). The real blocker is `WS2812SPI { led: spi }` — a struct literal, which the "constructor is a call on a plain path" rule refuses | **Not a bus gap**; struct literals stay out of KDL (Milestone 5) |
| 6 | 5 crates need ~15 lines of Rust glue | Correct. Builders, typestates and multi-object panels are not a manifest problem | **Out of scope** — a later `driver-glue.md` |

Two supporting facts that shape the design:

- **Arguments and the address are kept in two parallel lists that must stay
  aligned by position.** `recipe.rs:433-438` inserts the address at
  `factory_args[0]`; `model.rs:1372-1378` inserts it at `args[0]`; the deferred
  emitter then zips them (`model.rs:1447`). It works today only because the
  schema forces `{address}` to sit directly after the bus. Fixed arguments
  would make this worse, so Milestone 3 replaces both inserts with one ordered
  list.
- **A manifest-only driver's constructor error cannot be spelled by espforge,**
  so it is mapped to `DeviceErrorKind::Init`
  (`espforge-elements/src/model.rs:1411-1431`). That mapping must be skipped
  entirely for an infallible constructor (Milestone 2) — `.map_err` on a
  non-`Result` is the compile error that makes the "12/20" table wrong today.

Every milestone is additive to `format_version 2`: absent keys preserve
today's behaviour, so no `format_version 3` and no fixture migration.

---

## Milestone 1: Deferred Init for Manifest-Only Drivers

### Phase 1.1: `endpoint` Is a Rust Type, Not a Path
**Why:** the deferred factory stores the bus endpoint as a *type*
(`espforge-elements/src/devices/recipe.rs:458-465` parses it with
`optional_type`, and `model.rs:1446-1452` writes `endpoint: #endpoint_ty` as a
struct field). The schema, however, refuses anything with generics, so a
manifest can only name an alias it does not have. `espforge-drivers` gets away
with it by exporting `Scd4xI2cEndpoint`/`SpiEndpoint`; a manifest-only driver
cannot.

**Files to modify:**
- `espforge-schema/src/driver.rs` (`endpoint` handling in `parse_runtime`)
- `espforge-schema/src/lib.rs` (`parse_path_child` for the v1 `endpoint`)
- `espforge-schema/src/syntax.rs` (only if a shared validator is factored out)

**Steps:**
1. In `espforge-schema/src/driver.rs:1106-1118`, replace
   `if !is_rust_path(&value)` with `validate_type(&value, at.clone(), errors)`
   plus `validate_placeholders(&value, &[], at.clone(), "endpoint", errors)`.
   An endpoint may name a generic type but may take no `{placeholder}`: it is
   the type the generated bus helper already returns
   (`espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static,
   esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>`), not a template.
2. Update the error text: `` `endpoint` is not a Rust type `` with the
   `syn` error as the label, and
   `` `endpoint` takes no `{placeholder}` `` for the second check.
3. Apply the same change in `parse_path_child` (`espforge-schema/src/lib.rs`,
   used by the v1 `device_recipe` block) so a migrated v1 manifest is not held
   to a stricter rule than a v2 one.
4. Leave `is_rust_path` in place: the constructor path (`new`) still uses it,
   because a called function cannot carry generics as text.

**Verify:**
```sh
cargo test -p espforge-schema -p espforge-elements
```
Tests to add in `espforge-schema/src/driver.rs`:
- `an_endpoint_may_be_a_generic_type` — loads a driver whose
  `endpoint "CriticalSectionDevice<'static, I2c<'static, Blocking>>"` and
  asserts `recipe.endpoint` keeps the text.
- `an_endpoint_that_is_not_a_type_is_refused` — `endpoint "0x44"` reports
  `` `endpoint` is not a Rust type ``.
- `an_endpoint_takes_no_placeholder` — `endpoint "Endpoint<{bus}>"` reports
  the placeholder error, not a `syn` error.

### Phase 1.2: SHT31 Gets Its Deferred Mode Back
**Why:** the driver was written boot-only purely because of Phase 1.1; that is
also the regression test that keeps the new rule honest.

**Files to modify:**
- `espforge-drivers/manifests/sht31.kdl`
- `espforge-elements/src/schema.rs` (the manifest-only shape test)
- `espforge/tests/generated_projects_compile.rs` (new case)

**Steps:**
1. Restore `attr "init" kind="choice" default="boot" { choices "boot"
   "deferred" }` and the generics-bearing `endpoint` in the blocking runtime
   of `sht31.kdl`:
   ```
   endpoint "espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>"
   ```
   and the second example back to `init="deferred"`.
2. Extend `a_manifest_only_driver_names_an_external_crate` to assert the
   deferred example parses and that `recipe.endpoint` is the generic type.
3. Add a `generated_projects_compile` case built from
   `01.Basics/helloworld` with
   `<sht31 id="air" bus="$i2c0" address="0x44" init="deferred" />` on
   `esp32c3` blocking, which is the only place the factory struct is compiled.

**Verify:**
```sh
cargo test -p espforge-elements
cargo test -p espforge --test generated_projects_compile -- --ignored
```
The second command is the real proof: `Sht3x::new` takes `(i2c, address,
delay)`, so a wrong ordering in the factory call shows up as a type error in
`target/generated-check`.

---

## Milestone 2: Infallible Constructors

### Phase 2.1: `codegen { fallible #false }`
**Why:** the generated `Devices::new` and every deferred factory append
`.map_err(...)`, so a crate whose `new()` returns `Self` (not `Result`)
cannot be named by a manifest at all. `drivergap1.md` assumes this flag exists;
it does not.

**Files to modify:**
- `espforge-schema/src/driver.rs` (`parse_codegen` / driver assembly)
- `espforge-schema/src/lib.rs` (`DeviceRecipe`)
- `espforge-elements/src/lower.rs` (carry the flag into `Driver`)

**Steps:**
1. Add `pub fallible: bool` to `DeviceRecipe`
   (`espforge-schema/src/lib.rs:440-460`), documented as "the constructor
   returns a value; the generated code does not map its error".
2. Parse it with the existing `prop_flag(node, "fallible", true)` helper
   (`espforge-schema/src/driver.rs:588`) inside `codegen`, add it to the
   accepted-children list at `espforge-schema/src/driver.rs:1873-1885`, and
   default it to `true` for v1 elements and for every manifest that omits it.
3. Refuse `fallible #false` when `codegen.feature` is present: a bundled driver
   returns `Result<_, DeviceErrorKind>` and the `espforge-drivers` API would
   change shape. Message:
   `` `fallible #false` needs a crate that returns the device directly ``
   with the label `` drop `feature ".."` or the flag ``.
4. Carry the flag through lowering (`espforge-elements/src/devices/recipe.rs`
   `Driver` struct) so emission can read it.

**Verify:**
```sh
cargo test -p espforge-schema -p espforge-elements
```
Tests: `an_infallible_constructor_is_not_mapped` (parses, `recipe.fallible ==
false`), `an_infallible_bundled_driver_is_refused`, and `fallible_defaults_to_true`
on the `ssd1306` fixture.

### Phase 2.2: Emit Without an Error Mapping
**Why:** four emission sites assume a `Result`.

**Files to modify:**
- `espforge-elements/src/model.rs` (`BusDevice::emit`)
- `espforge-elements/src/lower.rs` (tests)

**Steps:**
1. Replace the single `fail` token with two tokens:
   - `fail` — today's `.map_err(..)`, used only when `driver.fallible`;
   - `check` — `#fail?` for a fallible constructor, empty otherwise.
2. Boot, blocking: `#name: #new(#handle, #(#args),*) #check,`.
3. Boot, embassy: the same token inside `AsyncMutex::new(..)`.
4. Deferred factory, blocking: body becomes
   `#new(#(#call_args),*) #fail` when fallible, `Ok(#new(#(#call_args),*))`
   when not.
5. Async deferred factory: `async move { #new(..).await #fail }` becomes
   `async move { Ok(#new(..).await) }` when infallible.
6. Keep the external-package error mapping from `model.rs:1411-1431` reachable
   only when `driver.fallible` — an infallible external constructor must never
   see `.map_err`.

**Verify:**
```sh
cargo test -p espforge-elements
```
Lowering tests: `an_infallible_constructor_needs_no_error_mapping` (the
`Devices::new` text has no `.map_err` for that device and the field is
assigned directly) and the deferred counterpart asserting `Ok(` in the factory
body.

### Phase 2.3: Prove It With a Real Crate
**Why:** text assertions do not prove that the constructor's shape matches the
emitted call.

**Steps:**
1. Add `espforge-drivers/manifests/sht4x.kdl` — `format_version 2`, no
   `feature`, `codegen { fallible #false }`, `type "sht4x::Sht4x<...>"`,
   `constructor "sht4x::Sht4x::new({bus_device})"`, `bus "i2c" { address
   default="0x44" }`. Verify the exact type parameters and the crate version
   against docs.rs before writing the manifest; `sht4x` is the smallest
   infallible-`new()` crate in the survey.
2. Register it in `espforge-drivers/src/lib.rs` (`MANIFESTS`,
   `DRIVERS_SCHEMA_KDL`, `TAGS`) with no module and no feature, exactly as
   `sht31` was.
3. Add a `generated_projects_compile` case: `helloworld` + `<sht4x id="air"
   bus="$i2c0" />` on `esp32c3` blocking.

**Verify:**
```sh
cargo test -p espforge --test generated_projects_compile -- --ignored
```

---

## Milestone 3: Fixed Constructor Arguments

### Phase 3.1: One Ordered Argument List
**Why:** the constructor tail is currently assembled by inserting the address
at index 0 in two different places (`espforge-elements/src/devices/recipe.rs:433-438`
and `espforge-elements/src/model.rs:1372-1378`) and then zipping two lists
(`model.rs:1447`). Fixed arguments must occupy a position, and position is
exactly what this scheme cannot express without another insert per feature.

**Files to modify:**
- `espforge-schema/src/lib.rs` (new `CtorArg`, `DeviceRecipe.args`)
- `espforge-schema/src/driver.rs` (construction sites)
- `espforge-elements/src/devices/recipe.rs` (`slots`)
- `espforge-elements/src/model.rs` (`emit`)
- `espforge-core/src/drivers.rs` (v1→v2 writer)
- `espforge-elements/tests/manifests.rs` (v1↔v2 equivalence)

**Steps:**
1. Add to `espforge-schema/src/lib.rs`:
   ```rust
   /// One positional argument of a driver's constructor.
   pub enum CtorArg {
       /// `{address}`: the element's I2C address, emitted as a `u8` constant.
       Address,
       /// A slot filled from an element attribute: a mapped value or a pin.
       Slot(DeviceArg),
       /// A fixed Rust expression the manifest writes verbatim.
       Fixed { text: String, span: Range<usize> },
   }
   ```
   and change `DeviceRecipe.args` to `Vec<CtorArg>`. `DeviceArg` itself does
   not change.
2. In `espforge-schema/src/driver.rs:1344-1393`, push `CtorArg::Slot(..)` where
   `DeviceArg` is pushed today and replace the `address_seen` removal
   (`:1320-1345`) with `CtorArg::Address` at the index the placeholder held.
   Keep the rule that `{address}` must directly follow the bus, with the
   existing message.
3. In `espforge-elements/src/devices/recipe.rs`, have `slots()` take the
   resolved `address: Option<u8>` and return `args`/`factory_args` of equal
   length in `CtorArg` order (`Address` → `RustExpr::number`), then delete the
   `factory_args.insert(0, ..)` block at `:433-438`.
4. In `espforge-elements/src/model.rs:1372-1378`, delete the `args.insert(0,
   ..)` for I2C, and make the deferred loop iterate `driver.factory_args`
   alone, taking the expression from `FactoryArg::Constant` instead of zipping
   with `args` (`:1447`). Two lists that must stay positionally aligned become
   one.
5. Update the v1→v2 migration writer (`espforge-core/src/drivers.rs:1505-1511`)
   to walk `CtorArg`: `Address` → `{address}`, `Slot` → `{name}`, `Fixed` →
   the text verbatim. This removes its own special-cased `args.push("{address}")`.
6. Update `espforge-elements/tests/manifests.rs:62-69`, which compares v1 and
   v2 argument lists by name: compare `CtorArg` for kind and, for `Slot`, name,
   map and pin as today.

**Verify:**
```sh
cargo test -p espforge-schema -p espforge-elements -p espforge-core
cargo test -p espforge-elements --test manifests
cargo test -p espforge --test golden_projects
```
The goldens must come out byte-identical: this phase is a refactor. Any golden
diff means the address or an argument moved.

### Phase 3.2: Parse Fixed Arguments
**Why:** a constructor argument that is not a `{placeholder}` is a value the
manifest already knows — `ADS111x::new(i2c, 0x48, ADS111xConfig::default())`,
`Bme680::new(i2c, DeviceAddress::Primary, delay, &Configuration::default(), 20)`.
Requiring a `map` for these would mean inventing an attribute the user must
never write.

**Files to modify:**
- `espforge-schema/src/driver.rs` (`parse_constructor`, `ConstructorCtx`)

**Steps:**
1. Split `parse_constructor`'s argument loop: keep the current branch for
   `Expr::Path` sentinels; add a branch for every other `syn::Expr` that is
   accepted as `CtorArg::Fixed { text, span }`.
2. Validate each fixed expression with `syn::parse_str::<Expr>` on the
   substituted text and keep the original text (holes would be sentinels; a
   fixed argument may not contain one — see Phase 3.4).
3. Keep the "bus first" rule: the first argument must still be
   `{bus_device}`/`{spi_device}`.
4. Keep the address rule: `{address}` must sit directly after the bus, and a
   fixed argument may appear anywhere after it.

### Phase 3.3: Emit Fixed Arguments
**Why:** lowering already turns one argument into one `RustExpr` and one
`FactoryArg::Constant`; fixed arguments must ride the same path so boot,
deferred and async all agree.

**Files to modify:**
- `espforge-elements/src/devices/recipe.rs` (`slots`)
- `espforge-schema/src/driver.rs` (async/blocking shape comparison)

**Steps:**
1. In `slots()`, add the `CtorArg::Fixed` arm: `RustExpr::parse(text)` (report
   `DEVICE_INVALID` with the schema span if it ever fails) pushed to `args`
   and `FactoryArg::Constant(expr)` pushed to `factory_args`.
2. Delete the `_ =>` arm's assumption that a non-pin argument has a `map`;
   `CtorArg::Slot` keeps the current `mapped(..)` behaviour.
3. In the async/blocking comparison
   (`espforge-schema/src/driver.rs:1412-1428`), compare slot names as today
   and additionally require the fixed arguments to match textually — a
   dual-mode driver whose two constructors take different constants is a
   manifest bug.

### Phase 3.4: Refuse What Cannot Be Spelled
**Steps:**
1. A fixed argument is an expression, not a template: reject any remaining
   `{name}` after substitution with
   `` a fixed constructor argument takes no `{placeholder}` ``.
2. Reject a fixed argument that is a bare `{`-less path to nothing? No — a
   path to a constant (`bme680::DeviceAddress::Primary`) is legal and common.
3. Report at the constructor span, one diagnostic per bad argument, so a
   manifest with two mistakes shows both.

**Verify:**
```sh
cargo test -p espforge-schema -p espforge-elements
```
Tests:
- `a_fixed_argument_must_be_a_rust_expression` — `constructor "d::D::new({bus_device}, 2 +)"`.
- `a_fixed_argument_takes_no_placeholder` — `constructor "d::D::new({bus_device}, {x})"`.
- `a_fixed_argument_keeps_its_position` (lowering) — a manifest with
  `constructor "d::D::new({bus_device}, {address}, d::Cfg::default(), 20)"`
  and one mapped slot emits the call in that exact order, boot and deferred.
- `dual_mode_constructors_must_take_the_same_constants` (schema).

### Phase 3.5: Prove It With the Two Crates the Survey Names
**Steps:**
1. Add `espforge-drivers/manifests/ads111x.kdl` using
   `embedded-ads111x` with `constructor "ads111x::Ads111x::new({bus_device},
   {address}, ads111x::Ads111xConfig::default())"`. Check the crate's real
   module path and config name on docs.rs first; `drivergap1.md` notes 0.1.3
   pulls `maybe-async-cfg ^0.2.5`, so pin a version that compiles.
2. Add `espforge-drivers/manifests/bme680.kdl` using `bosch-bme680` with
   `constructor "bme680::Bme680::new({bus_device}, bme680::DeviceAddress::Primary,
   {delay}, &bme680::Configuration::default(), 20)"`.
3. Register both in `espforge-drivers/src/lib.rs`; add one
   `generated_projects_compile` case each on `esp32c3` blocking.
4. Update the tally in `drivergap1.md` to point at this document rather than
   restating a count that depends on unimplemented flags.

**Verify:**
```sh
cargo test -p espforge --test generated_projects_compile -- --ignored
```

---

## Milestone 4: Per-Runtime Dependency Features

### Phase 4.1: Declare Both Feature Sets
**Why:** `stts22h-rs` and `bmi323-driver` put blocking and async support in
mutually exclusive Cargo features. A single `features` list forces both on, and
the crate then fails to compile. `default_features` is already parsed
(`espforge-schema/src/driver.rs:531`, `espforge-schema/src/lib.rs:206`) but
manifest-only packages ignore their own block, because
`Dependency::from_package` hard-wires `default_features: true`
(`espforge-schema/src/lib.rs:225-231`).

**Files to modify:**
- `espforge-schema/src/lib.rs` (`Dependency`)
- `espforge-schema/src/driver.rs` (`parse_dependency`)
- `espforge-elements/src/model.rs` (`Board::driver_dependencies`)

**Steps:**
1. Add to `Dependency`:
   ```rust
   /// Features that only apply when the device's bus resolves async.
   pub async_features: Vec<String>,
   /// True when the crate's blocking and async features must not both be on.
   pub modes_are_exclusive: bool,
   ```
2. Parse `async_features "async"` and `modes_are_exclusive #true` inside the
   `dependency` block (`espforge-schema/src/driver.rs:520-586`), extend the
   accepted-children list and the error text there.
3. Make `Dependency::from_package` keep the package's own `features` and
   `default_features` instead of forcing `true`; a manifest that writes
   `default_features #false` must reach `Cargo.toml`.
4. Extend `absorb` (`espforge-schema/src/lib.rs:270-285`) to union
   `async_features` and to AND `modes_are_exclusive`.

### Phase 4.2: Resolve per Bus Mode
**Why:** which feature set applies is a property of the board, not of the
manifest: the same driver on an embassy project may resolve async.

**Steps:**
1. In `Board::driver_dependencies` (`espforge-elements/src/model.rs:1752`),
   when `self.bus_mode(device.bus()) == BusMode::Async`, merge the dependency's
   `async_features` into its `features` before absorbing.
2. Keep the blocking set as the base, so a blocking project emits exactly what
   it emits today.

### Phase 4.3: Refuse Both Modes of an Exclusive Crate
**Why:** Cargo features are additive. If one project needs both the blocking
and the async feature of the same crate, the crate is compiled with both and
the manifest cannot be honoured. That must be a named error, not a cascade of
upstream compile failures.

**Files to modify:**
- `espforge-elements/src/lower.rs`
- `espforge-diagnostics/src/codes.rs`

**Steps:**
1. Add `check_exclusive_driver_modes(cx)` after the device section lowers,
   mirroring the I2C address-collision check, and report the two device ids,
   the package, and both bus modes with a new code (`E0911`, "driver modes
   conflict").
2. Only fire when `modes_are_exclusive` is `true`; merging is legal otherwise.

**Verify:**
```sh
cargo test -p espforge-schema -p espforge-elements -p espforge-emit
```
Tests:
- `async_features_reach_the_dependency_only_for_async_buses`
  (`espforge-emit/src/codegen/cargo.rs` tests, comparing the generated
  `Cargo.toml` for a blocking and an embassy board).
- `a_manifest_default_features_false_is_honoured` — a manifest-only package
  with `default_features #false` emits `default-features = false`.
- `two_modes_of_one_exclusive_crate_are_refused` (lowering, `E0911`).
- `an_async_feature_on_a_blocking_board_adds_nothing`.

### Phase 4.4: Prove It With `stts22h-rs`
**Steps:**
1. Add `espforge-drivers/manifests/stts22h.kdl` with
   `dependency "stts22h-rs" { version "..", default_features #false,
   features "blocking", async_features "async", modes_are_exclusive #true }`,
   a `map "address"` for `I2CAddress`, and both runtimes.
2. Register it; add two `generated_projects_compile` cases (blocking and
   embassy) so both feature selections are compiled.
3. Verify the feature names against the crate's `Cargo.toml` before writing the
   manifest — the survey's names (`blocking`/`asynchronous`) must be confirmed.

**Verify:**
```sh
cargo test -p espforge --test generated_projects_compile -- --ignored
```

---

## Milestone 5: What Stays Out of KDL

Three limitations are deliberately left as Rust glue; writing them as KDL
templates would turn the manifest into a macro language.

1. **Struct-literal constructors** (`WS2812SPI { led: spi }`). The generated
   call is `#new(#handle, ..)`, and a literal has no `new` path. Extending the
   template to arbitrary expressions would make `constructor` a second
   programming language. A dedicated `led-strip` manifest that names a
   one-line constructor in a crate of its own is the honest answer.
2. **Chip-select-less SPI.** `ws2812` needs `cs` to be optional for *a device*,
   not for the bus. `cs_via "pin_or_ref"` (`ili9341.kdl`) already models the
   existing shapes; a device with no chip select is a bus-level decision that
   belongs with Wave 3 `provides`, not here.
3. **Builders and typestates** (`hd44780-embedded-hal`, `epd-waveshare-async`,
   `epdsi`, `icm20948-async`, `lora-phy`). Each needs 10–20 lines that compose
   three or four upstream objects and, in two cases, run an `async`
   initialization. Plan them as a follow-up `driver-glue.md` with one small
   Rust function per crate — not one wrapper struct per crate.

Record the decision in `drivergap1.md` (one paragraph pointing here) so the
next reader does not re-open it.

---

## Milestone 6: Documentation and Final Gate

**Steps:**
1. Extend the manifest reference in `docs/` (the page that documents
   `format_version 2`) with: `codegen { fallible #false }`, generic
   `endpoint`, fixed constructor arguments, and the four new `dependency`
   keys.
2. `espforge driver explain <tag>` (if it prints the recipe) must show the new
   fields; check the renderer still compiles with `DeviceRecipe.args` typed as
   `Vec<CtorArg>`.
3. Cross-link `drivergap1.md` → `driverfixes.md` and note that its 12/20 count
   assumes Milestone 2.

**Final gate** — the same four-condition gate as `drivers.md`, plus the
manifest fixture suite:

```sh
cargo test --workspace
cargo test -p espforge-elements --test manifests
cargo test -p espforge --test golden_projects -- --ignored
cargo test -p espforge --test generated_projects_compile -- --ignored
cargo test -p espforge --test compile_fail -- --ignored
git diff --exit-code   # after reviewing any regenerated goldens
```

Expected golden churn: none for Milestones 1, 2 and 4 (no example uses the new
keys); **none** for Milestone 3, which is a refactor — a diff there is a bug.

---

## Acceptance Criteria

- `espforge-drivers/manifests/sht31.kdl` compiles with
  `init="deferred"` on `esp32c3` blocking, with no `espforge-drivers` feature
  and no `espforge-drivers` dependency line in the generated `Cargo.toml`.
- A manifest-only driver with `fallible #false` compiles, and its
  `Devices::new` contains no `.map_err` for that device.
- `ADS111xConfig::default()`, `&Configuration::default()` and `20` survive as
  constructor arguments in the emitted call, in position, for boot, deferred
  and async.
- `Dependency` can select blocking vs async features per board, honours
  `default_features #false`, and refuses both modes of an exclusive crate with
  a named diagnostic.
- `cargo test --workspace` is green, the golden trees are byte-identical, and
  the manifest v1↔v2 equivalence test still passes.
