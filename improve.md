# EspForge: three architecture improvements

**Audience:** a developer (or agent) picking this up cold.
**Goal:** make the code easier to read and safer to change, without changing user-visible behaviour.
**Status:** proposal only. No code has been changed for this document.

---

## How to use this document

The three improvements are independent and each is shippable on its own. They are ordered by
leverage: #1 deletes the most code, #2 fixes the highest-cost coupling, #3 removes the most
day-to-day friction.

| # | Improvement | Seam | Mostly touches | Scale |
|---|-------------|------|----------------|-------|
| 1 | One pin/resource ownership ledger | `espforge-hardware` | `conflicts.rs`, `components/*` | Delete ~400 lines, add ~150 |
| 2 | One declared vocabulary for peripherals & components | `espf` → `espforge-model` → `espforge-bindings` → `espforge-hardware` | `espf/src/context.rs`, `bindings/src/components/*`, `hardware/src/lib.rs` | Move/delete ~700 lines |
| 3 | One command outcome type + pipeline façade | `espforge` CLI | `main.rs`, `commands/*` | Six error conventions → one |

Each section follows the same shape: **evidence → why it hurts → target design → step-by-step
migration → acceptance criteria → risks.** Section 4 lists the small cleanups that are *not* part
of the top three but are cheap wins once the seams exist. Section 5 gives a work order.

**Ground rule for every step:** diagnostics are part of the product. Every refactor below must
leave the rendered `error:` text, the offending span, and the exit code unchanged. Where that is
impossible, the change is called out explicitly and the affected tests are named.

---

## 0. Where we are

### 0.1 The workspace

Ten crates, ~18.1k lines of Rust (tests included):

```
espf                        2.2k   ESPF syntax: parser, value tree, writer, diagnostics, `$` graph
espforge-model                287   Meta/Runtime/Console/Artifact/Hardware specs (leaf crate, no deps)
espforge-options            3.1k   options.kdl schema + resolver, chips, `# espforge` reader, element catalog
espforge-bindings             880   typed `<*_component>` wrappers, driver registry (build.rs generated)
espforge-hardware           2.4k   validated sections -> Hardware (per-bus lowering, conflicts)
espforge-emit               2.3k   minijinja context, template plan/render, manifest, disk writes
espforge-espgenerate-template 451   embedded project template + options.kdl + elements.kdl
espforge-examples             788   embedded example projects
espforge-runtime            1.3k   no_std runtime support for generated firmware
espforge                    4.5k   CLI (clap), commands, help rendering
```

### 0.2 The pipeline as it actually runs today

```
 source.rs ──► frontend::load ──► pipeline::validate ──► pipeline::hardware ──► emit::generate ──► emit::write_all
 (file or        parse + junk +      flags over file,       hardware::lower        minijinja            disk
  example)       # espforge core     options resolve,       (peripherals,          render +
                 + <options>         `$` graph bind          components, wifi)      overlay
```

The seams are already in the right places. `source.rs` owns file discovery, `frontend.rs` owns
parse-and-structure, `pipeline.rs` owns "same validation for `check` and `build`", and
`espforge-hardware` is a pure library. The problems below are *inside* those seams, not a missing
architecture.

### 0.3 Baseline before touching anything

```bash
cargo check --workspace --all-targets   # clean (warnings only)
cargo test  --workspace                 # NOT green at baseline
```

`cargo test --workspace` currently fails 3 tests, all in `espforge-emit`:

| Test | Failure | Cause |
|------|---------|-------|
| `context::tests::leds_are_reachable_by_list_and_by_name` | expects `"false"`, gets `"False"` | minijinja bool display |
| `context::tests::hyphenated_options_are_reachable_by_index` | expects `"false"`, gets `"False"` | same |
| `project::tests::jtag_console_and_description_reach_cargo_toml` | `unknown filter: tojson` | minijinja `json` feature not enabled |

**Do this first.** A refactor with a red baseline cannot prove it preserved behaviour. Either fix
these three (preferred — the `tojson` one is a genuine template bug) or mark them `#[ignore]` with
an issue reference *before* starting, so "no new failures" is a meaningful gate.

Existing warning noise to clear as well (they hide new warnings):
`espforge-hardware` lib tests 6, `espforge-emit` 1, `espf` 1, plus `unused variable: device` /
`component` in the generated driver registry.

### 0.4 How to prove behaviour was preserved

Before step 1, add **one** characterization test (a few dozen lines) and keep it green for all
three workstreams:

```rust
// espforge-hardware/src/characterization.rs (or tests/diagnostics.rs)
// Table of (spec, expected rendered diagnostics). Render with
// espf::diag::render_errors("x.espf", spec, &errs) and assert the string.
// Cover at minimum the cases in Appendix B.
```

The existing tests already assert on message *fragments* (`err.contains("both use GPIO18")`), but
not on which of two possible conflicts wins. Fixing that gap is what makes #1 safe.

---

## 1. One pin/resource ownership ledger

> Replace 400 lines of nested pairwise loops and five copies of "this pin is taken" with one
> claim table that every lowering step writes to.

### 1.1 Evidence

Pin ownership is checked in six files, in four different shapes.

| Where | What it checks | Shape |
|-------|----------------|-------|
| `espforge-hardware/src/conflicts.rs` (403 lines) | every cross-kind pin clash, plus UART peripheral clashes | 6 nested `for` pyramids, each ~20 lines |
| `components/led.rs:67,77` | led vs led, led vs button | two `iter().find` |
| `components/button.rs:67,77` | button vs button, button vs led | two `iter().any/find` |
| `components/i2c_component.rs:44` | `<i2c>` already has a component | `draft.used.is_some()` |
| `components/spi_component.rs:71` | cs vs cs | `iter().find` |
| `components/uart_component.rs:42` | `<uart>` already has a component | `draft.component.is_some()` |
| `conflicts.rs:285-295` | `tx == rx` on one UART | inline `if` |

`conflicts.rs` has **no tests of its own**; it is only covered indirectly through
`espforge-hardware/src/lib.rs`'s `snapshot_tests`. Its structure is one block per pair:

```rust
for bus in spi_buses {
    for pin in bus_pins(bus) {
        for gpio in gpios { if gpio.pin == pin { return Err(conflict_err(..., "gpio", ...)); } }
        for led in leds   { if led.pin == pin { return Err(conflict_err(..., "led",  ...)); } }
        for button in buttons { ... }
        for i2c in i2c_buses { ... }
        for other in spi_buses { ... }
        for dev in spi_devices { ... }
    }
}
```

…and the same six-block shape is then repeated for chip-selects (`conflicts.rs:188`) and for both
UART pins (`conflicts.rs:297`). To answer "who else can own a pin?" you have to read three
unrelated pyramids and mentally union them.

`conflict_err` (line 27) also has a stringly-typed back-door: `first_tag: &str` matched against
`"gpio" | "i2c" | "spi" | "uart"` to decide whether to look the node up in `# peripherals` or
`# components`, with a silent fallback to `EspfError::at_span(0..0, ..)` when the lookup misses —
i.e. a wiring error can silently lose its source location.

### 1.2 Why it hurts readability

- **A new peripheral or component is a 6-place change.** Adding `<pwm>` means touching every
  pyramid so PWM pins are protected, plus the "unsupported" message in `hardware/src/lib.rs:79`.
- **The rules are implicit.** Nothing states that an I2C pin may not collide with a UART pin;
  it is inferred from the fact that the UART loop happens to include `i2c_buses`.
- **Priority is accidental.** Which of two real clashes is reported depends on loop nesting, not
  on a stated rule, and no test pins it down.
- **Messages are assembled 21 times.** Every block hand-writes the same
  `"<{a} id=\"{}\"> and <{b} id=\"{}\"> both use GPIO{pin}"` with different word order; three
  spellings exist (`'s cs`, `tx and rx cannot both be`, `both use <i2c id=..>`).

### 1.3 Target design

One ledger in `espforge-hardware`, written to by every lowering step, read by one `report()`.

```rust
// espforge-hardware/src/claims.rs
use espf::context::ParsedNode;
use espf::diag::EspfError;

/// What kind of thing owns a pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Gpio, I2cBus, Led, Button, SpiBus, SpiCs, Uart,
}

/// One claim on one physical pin.
#[derive(Debug, Clone)]
pub(crate) struct Claim {
    pub pin: u8,
    pub kind: Kind,
    /// Human phrase used verbatim in the message, e.g. `<led id="red_led">`
    /// or `<spi_component id="tester">'s cs`.
    pub label: String,
    /// Where to point the snippet. `EspfError::at_node` computes exactly this
    /// span (`offset..offset + 1 + tag_name.len()`), so snippets do not move.
    pub span: std::ops::Range<usize>,
    /// The bus/peripheral this claim belongs to, for the one "same bus" rule
    /// (a chip-select may not land on a pin of its own bus).
    pub owner: Option<String>,
}

impl Claim {
    fn new(kind: Kind, label: String, node: &ParsedNode, pin: u8) -> Self { .. }
}

#[derive(Default)]
pub(crate) struct Claims {
    claims: Vec<Claim>,               // insertion (= file) order
    first_on_pin: std::collections::HashMap<u8, usize>,
}

impl Claims {
    pub fn claim(&mut self, c: Claim) -> Result<(), EspfError>;
    /// The pin ledger, in the order #1.4 preserves.
    pub fn conflicts(&self) -> Option<Conflict>;
}
```

Design notes:

- **Ownership is a pin fact, not a component fact.** `<led>`, `<button>`, `<spi_component cs>`,
  `<i2c>`, `<spi>`, `<uart>` and `<gpio>` all claim pins; none of them needs to know about the
  others.
- **Labels are stored, not reconstructed.** Keeping the phrase on the claim means the message
  text is produced exactly once and the odd cases (`'s cs`, `tx and rx`) stay exact.
- **Two phases, deliberately.** Lowering *records* claims (and still reports the same-kind
  duplicates where it does today, so `led.rs`/`button.rs`/`spi_component.rs` keep their messages
  and their existing test expectations). Cross-kind conflicts are reported once, after lowering,
  by `claims::conflicts()`. This mirrors today's `check_all` position in `lower()` and keeps the
  "first error in file order" property.
- **Spans, not node references.** The ledger stores `Range<usize>` (everything in `espf::diag`
  already speaks byte spans) so it has no lifetime and no borrow on the parsed nodes. Build the
  error with `EspfError::at_span`.

The check becomes a table instead of a pyramid. Transcribe today's order exactly:

| Sweep | Later claim kind | May not collide with | Today's code |
|-------|------------------|----------------------|--------------|
| 1 | `I2cBus` (sda, scl) | `Gpio` | `conflicts.rs:74` |
| 2 | `SpiBus` (sclk, mosi, miso) | `Gpio`, `Led`, `Button`, `I2cBus`, `SpiBus` | `:94` |
| 3 | `SpiBus` | `SpiCs` of the *same* bus | `:168` |
| 4 | `SpiCs` | `Gpio`, `Led`, `Button`, `I2cBus`, `SpiBus` | `:188` |
| 5 | `Uart` (tx, rx) | `Gpio`, `Led`, `Button`, `I2cBus`, `SpiBus`, `SpiCs`, `Uart` | `:297` |
| 6 | `Uart` tx vs its own rx | (same claim owner) | `:285` |

```rust
pub(crate) fn conflicts(claims: &Claims) -> Option<Conflict> {
    for sweep in SWEEPS {                       // table above, as data
        for later in claims.of_kind(sweep.later) {
            for earlier in claims.on_pin(later.pin) {   // HashMap lookup, O(1)
                if sweep.may_not_collide.contains(&earlier.kind)
                    || sweep.same_bus_only(earlier, later)
                {
                    return Some(Conflict::new(earlier, later));
                }
            }
        }
    }
    None
}
```

Two details make this a behaviour-preserving port:

1. **Iterate in the same order as the loops do today.** `claims.of_kind` must yield claims in
   insertion order, and peripherals are lowered before components (`lower()` already does this).
   Pins of one bus must be inserted `sclk, mosi, miso` — the order `bus_pins()` returns.
2. **Preserve kind priority within a pin.** Today a bus pin that clashes with both a `<gpio>` and a
   `<led>` reports the `<gpio>` (gpio is the first inner loop). So `on_pin` returns claims sorted
   by the sweep's declared kind order, not by insertion order. Encode that order in the table.

`Conflict::new` is the single formatter:

```rust
impl Conflict {
    fn error(&self) -> EspfError {
        EspfError::at_span(
            self.earlier.span.clone(),
            "pin conflict",
            format!("{} and {} both use GPIO{}", self.earlier.label, self.later.label, self.pin),
        )
    }
}
```

Check against Appendix B: every existing message is `"<A> and <B> both use GPIO{n}"` once
`SpiCs.label` is `"<spi_component id=\"tester\">'s cs"`. The two exceptions stay special-cased and
are listed there.

### 1.4 Migration plan

Do these in order; each step compiles and keeps the suite green.

1. **Add the characterization test** (0.4) covering all Appendix B cases, including specs with
   *two* clashes where the winner matters.
2. **Add `claims.rs` with the data model only** (`Kind`, `Claim`, `Claims`, `claim()`,
   `on_pin`, `of_kind`). No callers yet. Unit-test `claim()` returning the same-kind error.
3. **Convert `conflicts.rs` to the ledger.** Build the `Claims` from the drafts inside `check_all`
   (no signature change yet: `check_all` keeps taking drafts), implement `SWEEPS`, and delete the
   six pyramids. `check_all` shrinks to ~30 lines. This is the big readability win and the
   safest first move — nothing else changes.
4. **Move claim recording into lowering.** `lower_gpio`, `lower_i2c`, `lower_spi`, `lower_uart`
   and the `attach_*` functions take `&mut Claims` and claim their pins as they validate.
   Now `check_all`'s arguments (`gpios`, `leds`, `buttons`, …) disappear; it takes `&Claims`.
   Fold `#[allow(clippy::too_many_arguments)]` on `check_all` away with it.
5. **Merge the per-component duplicates** (led-vs-led, button-vs-button, cs-vs-cs) into
   `claim()`: same-kind collision on one pin is already the same question. Keep the existing
   messages; they differ only in the noun (`both use GPIO{n}`), which the label supplies, and in
   the `pin direction clash` title, which stays a separate check in `led.rs`/`button.rs`.
6. **Delete the now-dead plumbing:** `conflict_err`, `peripheral_node`, `component_node`, and the
   `first_tag: &str` dispatch. If a node lookup cannot be resolved, that is a bug — return a real
   error rather than `at_span(0..0, ..)`.

### 1.5 Acceptance criteria

- `espforge-hardware/src/conflicts.rs` is ≤ ~40 lines, contains no nested loops over element
  kinds, and no stringly tag dispatch.
- `lower()` body is unchanged except that `check_all` takes `&Claims`.
- Every case in Appendix B renders byte-identically (title, label, span/location, source line).
- The characterization test fails if you swap two entries of `SWEEPS`. (Try it once, then revert.)
- `cargo test --workspace` has no new failures.

### 1.6 Risks

- **Priority drift.** The one real hazard. Mitigated by the characterization test and by
  transcribing `SWEEPS` from the current code rather than from intuition.
- **Borrowing.** Lowering currently passes `&[GpioEntry]`, `&[LedSpec]` etc. into `attach_*`.
  Moving to `&mut Claims` alongside `&mut Vec<LedSpec>` is fine, but do not hold a `&Claim` across
  a `claim()` call — copy the fields you need.
- **`same_bus_only` (sweep 3)** is the one rule that needs a bus id on the claim; that is what
  `Claim::owner` is for. Do not special-case it in `conflicts()`.

---

## 2. One declared vocabulary for peripherals and components

> Today the same component is described four times: in `espf`'s graph, in the bindings wrappers, in
> the hardware lowering, and in `elements.kdl`. They drift, and the drift is patched with
> duplicated validation and stringly-typed tag names.

### 2.1 Evidence

**(a) The syntax crate knows hardware semantics.** `espf/src/context.rs` (352 lines) is not syntax:

```rust
pub enum ActivePeripheral { I2c { sda: u8, scl: u8 }, Spi, Gpio, Uart }   // :77
pub struct RuntimeContext<'a> {
    pub active_peripherals: Vec<Option<ActivePeripheral>>,                 // :90
    pub active_components:  Vec<Option<ActiveComponent>>,                  // :91
    pub resolved_devices:   Vec<ResolvedDevice<'a>>,                       // :92
}
// ...
active_peripherals: vec![None; 128],                                       // :99
```

`resolve_and_bind` (`:105`) hard-codes `"i2c" | "spi" | "uart" | "gpio"` tag names, parses I2C
`address` out of the raw attribute, and validates I2C `sda`/`scl` as `u8` — validation that
`espforge-hardware/src/peripherals/i2c.rs:47` performs again, with a different message.

Consequences:

- **Hard cap + panic.** 128 fixed slots indexed by `handle.0 as usize` (`:140`, `:187`) or by
  `handle.0` in `bindings`. A spec with 129 peripherals (or 129 components) panics with an
  index-out-of-bounds instead of reporting a diagnostic. `parse_u8_attr` (`:259`) has an
  `unreachable!()` on the same path.
- **Unused data.** `ActivePeripheral::I2c { sda, scl }` is only ever matched as `{ .. }`; the pin
  values are discarded. Validated twice, used zero times.
- **Not a graph.** `$` references are resolved by scanning attributes for known keys, and the
  result is three parallel vectors indexed by interned id.

**(b) The wrappers re-do the same work.** `espforge-bindings/src/components/*.rs` (~620 lines for
six wrappers) each: check `id`, fetch attributes, re-parse values, look up `ctx.interner`, index
`ctx.active_peripherals`, re-check the slug, and return `Result<Self, String>`. The bus wrappers
(`i2c_component`, `spi_component`, `uart_component`) prefix those strings with
`"❌ [i2c_component] ..."` — a format that appears nowhere else in the tool's diagnostics, and
with no span to point at.

`is_rust_ident` exists **four** times: the canonical `espforge-model/src/hardware.rs:114`, plus
private copies at `bindings/src/components/i2c_component.rs:84`,
`.../led_component.rs:91`, `.../button_component.rs:91`.

**(c) The lowering re-checks the wrapper.** Each `attach_*` in `espforge-hardware/src/components/`
calls `XComponent::resolve(...)` and then *again* builds the slug and calls `is_rust_ident`
(`led.rs:57`, `button.rs:57`, `i2c_component.rs:55`, `spi_component.rs:61`,
`uart_component.rs:110`), with a comment explaining that the duplicate exists only to keep the
message byte-identical.

**(d) The tag list is declared three times.** `espforge-bindings/src/components/mod.rs:28`
(`TAGS`), `resolve_tag_required_attrs` (`:37`), the `resolve_component` match (`:57`), plus
`drivers/generated_registry.rs` (build.rs) and `elements.kdl`. They are kept in sync by four
"drift test" assertions in `commands/espf.rs:220-260` — tests that exist *because* the single
source of truth is missing.

**(e) Adding one component is a five-file change** (wrapper, `TAGS`, required-attr match, catalog
entry, lowering dispatch), plus editing the long "only <led>, <button>, …" strings in
`hardware/src/lib.rs:79,118` and `devices/mod.rs:26`.

**(f) Dead scaffolding in the same area.** These are not architectural points but they show the
duplication is already hurting:

| Path | Lines | State |
|------|-------|-------|
| `espforge/src/sections/espforge.rs` + `mod.rs` | 232 | **never compiled** — `main.rs` declares `mod commands; mod help_view; mod source;` only. A stale copy of `espforge-options/src/section.rs`, including an `esp32`+`jtag-serial` rule that now lives in `options/src/resolve.rs:548`. |
| `espf/build.rs` + `espf/src/generated_registry.rs` | 61 | **dead** — generates an empty `route_driver` into a file `espf/src/lib.rs` never declares; there is no `espf/src/drivers/`. Copied from `espforge-bindings/build.rs`. |

### 2.2 Why it hurts readability

- **"Where is a component defined?" has four answers.** Reading one wrapper tells you half the
  truth; the lowering tells you the other half; `elements.kdl` tells you what the docs claim.
- **Tag names are strings in `match` arms** in four crates. A typo is a runtime "unsupported"
  message, not a compile error.
- **Two diagnostic vocabularies.** `"❌ [i2c_component] 'c' missing required attribute 'i2c'"`
  (a bare string, no span, no snippet) versus the rustc-style snippets everywhere else. Callers
  paper over it with `map_err(|m| EspfError::at_node(n, "invalid i2c_component", m))`, so the
  *title* changes depending on which layer caught the problem.
- **`espf` cannot be reused or reasoned about as a parser.** It depends on hardware vocabulary,
  and the CLI's `check` reaches into `ctx.resolved_devices` for driver routing.

### 2.3 Target design

Three layers, each with one job:

```
espf                syntax only: parse, spans, value tree, writer, diagnostics
                    ParsedNode/TagAttribute/AttrOp/Handle/IdInterner, NO tag vocabulary

espforge-model      the domain: Meta, Hardware specs, and the *declared vocabulary*
                    PeripheralDef/ComponentDef tables (tag, parent, required attrs, buildable)

espforge-bindings   the resolved graph + typed wrappers + driver registry
                    Graph<'a> { nodes, by_id, refs, kinds }, LedComponent, ...

espforge-hardware   lowering + claims; consumes Graph and wrappers, never re-validates
```

**(a) The vocabulary is data, in `espforge-model`.**

```rust
// espforge-model/src/vocab.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parent { Gpio, I2c, Spi, Uart }

pub struct ComponentDef {
    pub tag: &'static str,
    pub parent: Parent,
    /// Attributes required besides `id`. The catalog drift test reads this.
    pub required: &'static [&'static str],
    pub buildable: bool,
}

pub struct PeripheralDef {
    pub tag: &'static str,
    pub buildable: bool,
}

pub const COMPONENTS: &[ComponentDef] = &[
    ComponentDef { tag: "led",            parent: Parent::Gpio, required: &["gpio"],                 buildable: true },
    ComponentDef { tag: "button",         parent: Parent::Gpio, required: &["gpio"],                 buildable: true },
    ComponentDef { tag: "i2c_component",  parent: Parent::I2c,  required: &["i2c", "address"],       buildable: true },
    ComponentDef { tag: "spi_component",  parent: Parent::Spi,  required: &["spi", "cs"],            buildable: true },
    ComponentDef { tag: "uart_component", parent: Parent::Uart, required: &["uart", "baud_rate"],    buildable: true },
];

impl ComponentDef {
    pub fn get(tag: &str) -> Option<&'static Self>;
    pub fn supported_list() -> String;   // "only <led>, <button>, ..." from the table
}
```

Everything currently repeated becomes a derived value: `bindings::components::TAGS`,
`resolve_tag_required_attrs`, the `resolve_component` match, the "unsupported component" messages
in `hardware/src/lib.rs:118` and `devices/mod.rs:26`, and the drift tests in `commands/espf.rs`
(which become a loop over `COMPONENTS`).

**(b) The graph moves out of `espf`.**

```rust
// espforge-bindings/src/graph.rs
pub struct NodeId(usize);

pub struct Graph<'a> {
    nodes: Vec<&'a ParsedNode<'a>>,           // peripherals ++ components ++ devices
    by_id: HashMap<&'a str, NodeId>,          // one namespace, duplicate ids are an error here
    parents: HashMap<NodeId, NodeId>,         // "$" refs, resolved generically
    address: HashMap<NodeId, u8>,             // I2C address, parsed once
}

impl<'a> Graph<'a> {
    /// Replaces RuntimeContext::resolve_and_bind. Collects every error with a span.
    pub fn build(sections: &'a HashMap<String, Vec<ParsedNode<'a>>>) -> Result<Self, Vec<EspfError>>;
    pub fn parent(&self, id: NodeId) -> Option<NodeId>;
    pub fn id_of(&self, node: &ParsedNode) -> Option<NodeId>;
}
```

`Graph` is generic: it resolves *any* attribute whose value starts with `$` against the id
namespace. It does not care that `i2c` is a bus. The `Parent` enum on the definition supplies the
type check (`<led>`'s parent must be a `<gpio>`), and the check happens once, in the wrapper.

**(c) Wrappers produce located errors and nothing else.**

```rust
impl<'a> I2cComponent<'a> {
    pub fn resolve(node: &'a ParsedNode<'a>, graph: &Graph<'a>) -> Result<Self, EspfError>;
}
```

- Missing `address` → `EspfError::at_node(node, "invalid i2c_component", "needs address=\"...\"")`
  — same wording as today, but a snippet instead of a `❌ [..]` line, and the *caller* no longer
  wraps the string.
- Bad value → `EspfError::at_attr(attr, ..)`, which points at the value. (Today the wrapper throws
  the span away.)
- Parent kind mismatch → `at_attr(parent_attr, "expects an i2c", ..)` — keep the phrase
  "expects an i2c" so `i2c_component.rs` tests still pass, then tighten the title in one place.
- Slug check → `espforge_model::hardware::slug(node.id)` + `is_rust_ident`, once. Delete the three
  private copies and the five lowering-side repeats.

**(d) `lower()` dispatches from the table.**

```rust
for n in components {
    let Some(def) = ComponentDef::get(n.tag_name) else {
        return Err(vec![EspfError::at_node(n, "unsupported component", def_help(n.tag_name))]);
    };
    match def.tag {
        "led" => attach_led(n, &mut board, &claims, graph)?,
        "button" => attach_button(n, &mut board, &claims, graph)?,
        "i2c_component" => attach_i2c_component(n, &mut i2c_buses, graph)?,
        // ...
        _ => unreachable!("COMPONENTS and this match are drift-tested"),
    }
}
```

The match stays (each branch does different work), but the *list* and the *error message* come from
the table. Add a test that every `ComponentDef` has a branch, so the `unreachable!` is honest.

### 2.4 Migration plan

Each step compiles; the suite stays green.

1. **Delete the dead code first** (`espforge/src/sections/`, `espf/build.rs` +
   `espf/src/generated_registry.rs`). Zero risk, −293 lines, and it removes the most confusing
   duplicate of `core_from_section` before you start moving that code.
2. **Add `espforge-model/src/vocab.rs`** with `COMPONENTS`, `PeripheralDef`, `Parent`, and the
   `supported_list()` helpers. No callers. Port the drift tests from `commands/espf.rs` to assert
   `COMPONENTS` against `elements.kdl`, so the table is verified before it is trusted.
3. **Add `espforge-bindings/src/graph.rs`.** Port `resolve_and_bind` with three deliberate
   changes, each covered by a new unit test: no fixed-size tables (grow on insert), duplicate ids
   are a reported error instead of last-write-wins, and pin/address values are *not* parsed here.
   Keep a temporary `impl From<&Graph> for RuntimeContext`-style shim only if it keeps this step
   small; otherwise update the two driver call sites (`drivers/tmp102.rs`,
   `drivers/status_led.rs`) and `bindings/build.rs`'s generated signature in the same commit.
4. **Convert the wrappers** to `resolve(node, graph) -> Result<Self, EspfError>`, one per commit,
   deleting each private `is_rust_ident` as you go. Run `cargo test -p espforge-hardware` after
   each; the lowering tests assert on message fragments and will catch wording slips.
5. **Convert `lower()`** to take `&Graph` instead of `&RuntimeContext`, delete the five
   `is_rust_ident` repeats in `components/*`, and delete `RuntimeContext`,
   `ActivePeripheral`, `ActiveComponent`, `ResolvedDevice` from `espf` (move
   `ResolvedDevice`'s shape into `bindings::graph` if `check`'s driver routing still wants it).
6. **Derive the messages.** Replace the hard-coded "only <led>, <button>, …" strings with
   `ComponentDef::supported_list()`. Update the `commands/espf.rs` drift tests to iterate
   `COMPONENTS`.

### 2.5 Acceptance criteria

- `espf` contains no occurrence of `"i2c"`, `"spi"`, `"uart"`, `"gpio"`, `"address"`, `"sda"`,
  `"scl"` (grep-checkable). It exposes syntax, spans and diagnostics only.
- `is_rust_ident` has exactly one definition in the workspace.
- Adding a component requires exactly: one `COMPONENTS` entry, one wrapper module, one `attach_*`,
  one `match` arm, one `elements.kdl` entry. A new test asserts the first four agree.
- A spec with 129 peripherals (or 129 components) produces a normal diagnostic, not a panic.
  (New test.)
- No user-visible message loses its span; no message gains the `❌ [tag]` prefix.

### 2.6 Risks

- **Blast radius.** `RuntimeContext` is threaded through `pipeline::Checked`, `check`,
  `espf`, and the generated driver registry. Do step 3 behind a shim and convert callers in one
  compile-driven pass (`cargo check --workspace --all-targets` lists them all).
- **Driver signature.** `bindings/build.rs` generates `route_driver(tag, &ResolvedDevice,
  &RuntimeContext)`. If you move those types, change the generator first, then the two drivers.
- **Message text.** The wrappers currently own three messages that the lowering also produces.
  Pick one owner per message (wrapper) and delete the other; the tests asserting on fragments in
  `i2c_component.rs`, `spi_component.rs`, `led.rs`, `button.rs`, `uart_component.rs` are the
  contract.

---

## 3. One command outcome type + a pipeline façade

> The commands currently use six different error conventions, four of them print `❌` themselves,
> and `process::exit` is called from four places inside command bodies. Collapse to one.

### 3.1 Evidence

| Command | Returns | Prints `❌` at | Exit codes |
|---------|---------|---------------|------------|
| `build`/`gen` | `Result<(), build::Failure>` (`build.rs:64`) | `build.rs:85,94` | 1 / 2 |
| `new` | `Result<Created, new::Stop>` (`new.rs:76`) | `new.rs:369,395,403,408` | 1 / 2 / 130 |
| `check` | `Result<(), u8>` (`check.rs:31`) | `check.rs:36` | 1 |
| `espf` | `Result<(), u8>` (`espf.rs:30`) | `espf.rs:52` | 1 / 2 |
| `examples` (porcelain) | `io::Result<()>` (`examples.rs:46`) | `examples.rs:41` | 130 / 2 |
| `examples` (plumbing) | `anyhow::Result<()>` | `main.rs::finish` | 1 |
| `doctor` | `()` | — | `process::exit(1)` inside `doctor.rs:66` |
| `main` | `ExitCode` | `main.rs:166,177` | 1 / 2 |

`pipeline::Invalid` (`pipeline.rs:23`) is a *third* vocabulary, with `Invalid::Spans` vs
`Invalid::Message` and a `report()` that prints. So a failure can be printed at the point it is
detected, at the command boundary, or in `main`, depending on the command.

Two more consequences:

- **Repeated glue.** `invalid.report(&source.name, &source.text); Err(Failure::User)` appears three
  times; `for note in &checked.notes { eprintln!("{note}") }` twice; `commands::build::execute` is
  called directly from `new.rs:358` and `examples.rs:119`, so those commands inherit `build`'s
  error type as well as their own.
- **Exit-code documentation is copy-pasted** into the module docs of `build.rs`, `check.rs`, and
  `espf.rs`, and is already inconsistent (`check` documents only 0/1 but can print snippets and a
  warning).

### 3.2 Target design

One failure type, one printer, one exit-code map, and one façade that check/build/gen share.

```rust
// espforge/src/commands/outcome.rs
use std::process::ExitCode;

/// Why a command stopped.
pub enum Failure {
    /// Already reported (snippets / notes / a message the command printed).
    Reported,
    /// Print `❌ {msg}`; the user's input is at fault.
    Message(String),
    /// Print `❌ {e:#}`; espforge or the OS is at fault.
    Internal(anyhow::Error),
    /// The user cancelled an interactive prompt (SIGINT / Esc).
    Cancelled,
}

impl Failure {
    pub fn exit_code(&self) -> u8 {
        match self {
            Failure::Reported | Failure::Message(_) => 1,
            Failure::Internal(_) => 2,
            Failure::Cancelled => 130,
        }
    }
    /// The *only* place the tool prints `❌` for a command failure.
    pub fn report_and_exit(self) -> ExitCode { .. }
}

pub type Outcome = Result<(), Failure>;

impl From<anyhow::Error> for Failure { /* Internal */ }
impl From<std::io::Error> for Failure { /* Internal, but Interrupted -> Cancelled */ }
```

Then:

- `Invalid::report(self, name, src)` returns `Failure` (`Reported` when it printed, `Message` for
  the flag case) instead of printing inside `pipeline`. Or better: keep `report` and add
  `fn into_failure(self, name, src) -> Failure`.
- `build::Failure` and `new::Stop` are deleted; `build::run`, `new::run`, `check::run`,
  `espf::run`, `examples::run`, `doctor::run` all return `Outcome`.
- `main` becomes:

```rust
fn main() -> ExitCode {
    let cli = Cli::parse_from(expand_help(std::env::args_os().collect()));
    if let Err(f) = run(cli) { return f.report_and_exit(); }
    ExitCode::SUCCESS
}
```

with `fn run(cli: Cli) -> Outcome` holding the `match` that is in `main` today. `-C` failures
become `Failure::Message` (or `Internal`) instead of a second `eprintln!`.

- **The façade.** `pipeline` already owns "validate", "hardware" and "buildable". Add the one step
  every command repeats:

```rust
// espforge/src/commands/pipeline.rs
pub struct Prepared<'a> {
    pub source: Source,
    pub checked: Checked<'a>,
    pub hardware: Hardware,
}

/// resolve source -> validate -> lower hardware, reporting diagnostics on the way.
/// Every command that reaches code generation starts here.
pub fn prepare(
    file: Option<&Path>,
    example: Option<&str>,
    answers: &AnswerArgs,
) -> Result<Prepared<'_>, Failure>;
```

`check` becomes: `prepare` → driver routing → `buildable` warning → `println!("✓ …")`.
`build`/`gen` become: `prepare` → optional `--write-spec` → `emit::generate_with_hardware` →
`write_all` → copy `app.rs` → optional `cargo build`.
`new`/`examples` stop calling `build::execute` with a hand-built `BuildArgs` and instead call a
small `generate_only(source, out, answers)` that the façade exposes.

- **Exit-code contract** lives in one table in `main.rs`:

| Code | Meaning | Produced by |
|------|---------|-------------|
| 0 | success | — |
| 1 | user error (bad flag, invalid spec, drift refusal, not buildable) | `Reported`, `Message` |
| 2 | internal / IO failure (`cargo` missing, write failed) | `Internal` |
| 130 | cancelled at a prompt | `Cancelled` |

### 3.3 Migration plan

1. **Add `outcome.rs`** with `Failure`, `Outcome`, `From` impls, `exit_code`,
   `report_and_exit`. No callers. Unit-test the code mapping.
2. **Convert `check`** (smallest). `Result<(), u8>` → `Outcome`; delete its `eprintln!("❌ {m}")`
   by mapping the resolve error to `Failure::Message`. `main`'s `finish(...)` disappears for this
   command. Run `cargo test -p espforge`.
3. **Convert `build`/`gen`.** Delete `build::Failure`; `user(msg)` becomes
   `Failure::Message(msg)`; the drift-detection special case stays at the call site but returns
   `Failure::Message` from `msg`. `run` returns `Outcome`.
4. **Add `pipeline::prepare`** and rewrite `build::execute`, `check::execute` in terms of it.
   This deletes three copies of `invalid.report(...)` and both copies of the notes loop.
5. **Convert `new` and `examples`.** `Stop` → `Failure` (map `io::ErrorKind::Interrupted` to
   `Cancelled`); the `Ui` trait and `Scripted` test double stay untouched — they are good.
   Replace the two hand-built `BuildArgs { stage: Stage::Generate, .. }` calls with the façade.
6. **Convert `espf` and `doctor`.** `espf`'s `let Ok(text) = rendered else { eprintln!("❌ {}",
   rendered.unwrap_err()); … }` collapses to `rendered.map_err(Failure::Message)?`. `doctor`
   returns `Outcome` instead of calling `process::exit` in the middle of `run` (which also makes
   it testable).
7. **Delete `main::finish`** and the per-command "Exit codes:" doc paragraphs; point them at the
   table.

### 3.4 Acceptance criteria

- Exactly one `eprintln!("❌` for command failures (in `Failure::report_and_exit`).
- `std::process::exit` appears nowhere in `espforge/src/`; every command returns `Outcome` and
  `main` returns `ExitCode`.
- Two failure enums at most: `grep -rn "enum Failure\|enum Stop" espforge/src` finds one, and no
  command returns `Result<_, u8>`. `pipeline::Invalid` stays (it models *validation* failure, not
  process failure) and converts into `Failure`.
- `check`, `build`, `gen` share `pipeline::prepare`; `new` and `examples` do not construct
  `BuildArgs`.
- Exit codes observed in tests for each command match the table (add `assert_cmd`-style tests, or
  call `run()` and assert `Failure::exit_code()`).

### 3.5 Risks

- **`❌` lines are asserted by tests.** `commands/build.rs` and `commands/new.rs` tests mostly
  assert on `Result` variants, not on stderr, so this is low risk. Grep for `"❌"` in tests before
  moving anything.
- **`new`'s interactive flow** has its own `Stop::Io` handling for Ctrl-C. Map it to `Cancelled`
  and keep the `outro_cancel("Cancelled.")` call in `new::run` — cliclack owns that terminal
  state; do not move it into `report_and_exit`.
- **`pipeline::Invalid`.** Resist folding it into `Failure` in step 1: it carries
  `Vec<EspfError>` and needs `name`/`src` to render. It should convert *to* `Failure`, not *be*
  `Failure`.

---

## 4. Smaller cleanups (cheap once the seams exist)

These are not architectural, but each one deletes a lie the reader has to decode. They can be done
in any order, ideally interleaved as "boy scout" commits.

| Item | Location | Action | Saves |
|------|----------|--------|-------|
| Dead section reader | `espforge/src/sections/` (232 lines) | delete; `main.rs` never declares it | 232 lines + one stale duplicate of `core_from_section` |
| Dead driver registry in the parser crate | `espf/build.rs`, `espf/src/generated_registry.rs` (61) | delete both; `espf/src/lib.rs` never declares the module and there is no `espf/src/drivers/` | 61 lines + a confusing second `build.rs` |
| `render_project` duplicate | `espforge-emit/src/project.rs:72` | it is used only by tests; fold into `render_project_with_overlay(ctx, &[])` and have tests call that | 14 duplicated lines, one fewer public API |
| `DERIVED` const duplicated | `espforge/src/commands/new.rs:26`, `espforge-options/src/resolve.rs:27` | export `espforge_options::DERIVED` and use it in both | Two sources of truth for "you may not set this" |
| Private `is_rust_ident` copies | `bindings/src/components/{i2c,led,button}_component.rs` | use `espforge_model::hardware::is_rust_ident` and add `slug()` next to it | 3 copies (see #2) |
| Compat parser entry points | `espf/src/parser.rs:369,381` | `parse_espf_runtime` / `parse_espf_runtime_with_diagnostics` exist only for old call sites; replace with `parse_espf(...).sections` etc. | ~25 lines + one more way to parse a file |
| Migration-history comments | 34 occurrences of "step 1b", "item 2e", "step 2c pilot", "Moved verbatim", "plan 7.1" across 20 files | rewrite as present tense ("This module owns …"), keep the *why*, drop the itinerary | Reader no longer needs a plan document that isn't in the repo |
| Test modules larger than their module | `espf/parser.rs` (289 test lines / 674), `options/catalog.rs` (383/1410), `options/resolve.rs` (264/831), `emit/project.rs` (588/836), `emit/lib.rs` (498/652), `hardware/lib.rs` (94/297) | move to sibling `src/<mod>/tests.rs` (or `#[cfg(test)] mod tests;`), keeping module docs at the top | The production code of these files becomes readable in one screen |
| Warning noise | `cargo test` baseline | clear the 8 warnings and the unused `device`/`component` in the generated driver registry so a new warning is visible | — |
| `espf::value` docs drift | `value.rs` header says "the raw `ParsedNode::attributes` list stays untouched for callers (e.g. `RuntimeContext`) that scan it" | after #2, no such caller | — |

---

## 5. Suggested order of work

```text
0.  Fix / quarantine the 3 failing emits tests; clear warnings.        (½ day)
0.5 Add the diagnostics characterization test (Appendix B matrix).      (½ day)
1.  Pin ownership ledger.                                              (1–1.5 days)
    - claims.rs + SWEEPS, convert conflicts.rs              (safe half)
    - record claims during lowering, delete pairwise checks
2.  Vocabulary + graph.                                                (2–3 days)
    - delete dead sections/ + espf build.rs                (first, easy)
    - model::vocab + drift-test the table
    - bindings::graph, wrappers -> located errors
    - lower() takes &Graph, delete duplicated validation
3.  Command outcome + prepare().                                       (1–1.5 days)
    - outcome.rs, convert check, then build, then new/examples/espf/doctor
    - delete main::finish, add exit-code tests
4.  Small cleanups from §4 as each seam lands.
```

Why this order: #1 is self-contained and produces the largest line reduction per hour; #2 changes
the most types, so it wants a quiet tree and the characterization test in place; #3 is mechanical
and touches only the CLI, so it is safest at the end (and it will need small updates after #2
changes `Checked`'s graph type).

---

## Appendix A — evidence index

Every claim above, with its location, so a reviewer can check the document rather than trust it.

| Claim | Where |
|-------|-------|
| 403-line conflict checker with six nested pyramids, no own tests | `espforge-hardware/src/conflicts.rs` (checked at `:74, :94, :168, :188, :285, :297`) |
| Stringly tag dispatch in error construction | `conflicts.rs:27-47` (`peripheral_node`, `component_node`, `conflict_err`) |
| Same-kind pin checks duplicated per component | `components/led.rs:67,77`, `components/button.rs:67,77`, `components/spi_component.rs:71` |
| Bus occupancy checks duplicated per component | `components/i2c_component.rs:44`, `components/uart_component.rs:42` |
| Hardware semantics in the parser crate | `espf/src/context.rs:75, :105, :184` |
| Fixed 128-slot tables (panic above 128 ids) | `espf/src/context.rs:99-100`, indexed at `:140, :187` |
| Panic path in the diagnostics layer | `espf/src/context.rs:259` (`unreachable!`) |
| `ActivePeripheral::I2c { sda, scl }` values never used | matched as `{ .. }` in `bindings/src/components/i2c_component.rs:57` |
| I2C pin validation done twice, with different messages | `espf/src/context.rs:129-130` vs `hardware/src/peripherals/i2c.rs:40-60` |
| `is_rust_ident` × 4 | `model/src/hardware.rs:114`; `bindings/src/components/{i2c,led,button}_component.rs:84,91,91` |
| Slug re-checked in every `attach_*` | `hardware/src/components/{led:57,button:57,i2c_component:55,spi_component:61,uart_component:110}` |
| Tag list declared 3× + `elements.kdl` | `bindings/src/components/mod.rs:28,37,57`; `bindings/src/drivers/generated_registry.rs`; `espgenerate-template/src/elements.kdl` |
| Drift tests exist because of that | `espforge/src/commands/espf.rs:220-260` |
| Unsupported-component message hard-codes the list | `hardware/src/lib.rs:79,118`; `hardware/src/devices/mod.rs:26` |
| Dead section reader (never compiled) | `espforge/src/sections/`; `main.rs:1-3` declares only `commands`, `help_view`, `source` |
| Dead empty driver registry | `espf/build.rs`, `espf/src/generated_registry.rs`; `espf/src/lib.rs` |
| Five error conventions across commands | `build.rs:64`, `new.rs:76`, `check.rs:31`, `espf.rs:30`, `examples.rs:46/60`, `doctor.rs:66`, `main.rs:162` |
| Repeated glue in every command | `build.rs:142,146,155`, `check.rs:41,44,50` (`invalid.report` ×3, notes loop ×2) |
| `new`/`examples` call `build::execute` directly | `new.rs:358`, `examples.rs:119` |
| Baseline failing tests | `cargo test --workspace` → `espforge-emit` 3 failures (bool display, `tojson`) |
| Test-module sizes | `#[cfg(test)]` offsets: `parser.rs:385/674`, `catalog.rs:1027/1410`, `resolve.rs:567/831`, `project.rs:248/836`, `emit/lib.rs:154/652` |

## Appendix B — diagnostic contract

These strings must survive #1 and #2 unchanged (title and label), or the change must call out the
update and adjust the named tests. They are the behaviour the tests assert on.

**Pin conflicts** (`conflicts.rs`, `led.rs`, `button.rs`, `spi_component.rs`)

```
<gpio id="gpio{p}"> and <i2c id="{bus}"> both use GPIO{p}
<gpio id="gpio{p}"> and <spi id="{bus}"> both use GPIO{p}
<gpio id="gpio{p}"> and <spi_component id="{dev}"> both use GPIO{p}
<gpio id="gpio{p}"> and <uart id="{u}"> both use GPIO{p}
<led id="{led}"> and <spi id="{bus}"> both use GPIO{p}
<led id="{led}"> and <spi_component id="{dev}"> both use GPIO{p}
<led id="{led}"> and <uart id="{u}"> both use GPIO{p}
<button id="{btn}"> and <spi id="{bus}"> both use GPIO{p}
<button id="{btn}"> and <spi_component id="{dev}"> both use GPIO{p}
<button id="{btn}"> and <uart id="{u}"> both use GPIO{p}
<i2c id="{bus}"> and <spi id="{sb}"> both use GPIO{p}
<i2c id="{bus}"> and <spi_component id="{dev}"> both use GPIO{p}
<i2c id="{bus}"> and <uart id="{u}"> both use GPIO{p}
<spi id="{a}"> and <spi id="{b}"> both use GPIO{p}
<spi id="{bus}"> and <spi_component id="{dev}"> both use GPIO{p}
<led id="{led}"> and <led id="{other}"> both use GPIO{p}
<button id="{btn}"> and <button id="{other}"> both use GPIO{p}
<spi_component id="{dev}"> and <spi_component id="{other}"> both use GPIO{p}
<spi_component id="{dev}">'s cs and <spi id="{bus}"> both use GPIO{p}      ← special phrase
```

Note: the existing messages are inconsistent about which side is the raw id and which is the
slugged name (`<led id="{other}">` uses `other.name`, i.e. `red_led`, while the second side uses
`n.id`, i.e. `red-led`). The characterization test records the current spelling; do not "fix" it
as part of a refactor without saying so.

**Non-pin wiring** (keep their titles too)

```
title "pin direction clash"  <button id=".."> and <led id="..">: one pin cannot be both input and output
title "pin direction clash"  <led id=".."> and <button id="..">: one pin cannot be both input and output
title "i2c bus already used" <i2c_component id=".."> and <i2c_component id=".."> both use <i2c id="..">
title "uart already used"    <uart_component id=".."> and <uart_component id=".."> both use <uart id="..">
title "peripheral clash"     <uart id=".."> and <uart id=".."> both use UART{n}
title "uart wiring"          <uart id=".."> tx and rx cannot both be GPIO{p}
title "invalid component id" <TAG id="..">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)
```

**Wrapper phrases #2 must preserve while moving to spans**

```
"expects an i2c"      (i2c_component.rs test: a_gpio_sharing_a_pin_with_i2c_is_refused)
"expects an spi"      (spi_component.rs test: a_spi_component_needs_a_declared_spi)
"not a <gpio>"        (led.rs / button.rs tests)
"which is not a <gpio>"
"missing required attribute '{attr}'"   (spi_component.rs, uart_component.rs tests)
"must be \"true\" or \"false\""          (led.rs / button.rs tests)
"needs an output pin" / "needs an input pin"
"tx and rx"           (hardware/lib.rs snapshot_tests)
```

**Characterization matrix to write before starting** (one spec each, assert the full rendered
snippet):

1. led + button on one physical pin, both orders in the file
2. two leds on one pin
3. two buttons on one pin
4. gpio + i2c on the same pin
5. i2c + spi on the same pin
6. spi bus pin + led
7. spi cs + gpio
8. spi cs + its own bus pin
9. two spi buses sharing a pin
10. uart tx == rx
11. uart + gpio, uart + led, uart + i2c, uart + spi cs
12. two uarts on one peripheral
13. two i2c components on one bus
14. two uart components on one peripheral
15. a pin claimed by three things (asserts *which* pair is reported)

For each: `espf::diag::render_errors("x.espf", spec, &errs)` compared verbatim.
