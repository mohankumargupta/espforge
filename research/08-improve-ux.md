# Improving the ESPForge user experience

**Status:** product and documentation proposal  
**Baseline:** workspace `0.9.1`  
**Audience:** people who want firmware working on an ESP board and may not yet know the ESPForge file format, peripheral graph, Rust APIs, or embedded build toolchain.

## Summary

ESPForge already has a useful workflow for people who understand the project model: inspect examples, scaffold one, validate, generate, and build. The hard part is that the user must discover both the declarative hardware description in `.espf` and the imperative Rust behavior in `src/app.rs`. Today the CLI has a starter-example wizard, but no in-product way to discover a sensor's tag, wiring, options, supported chips/runtime, or the Rust calls they can make from `Context`.

This document proposes independent ways to close that gap:

1. **Documentation first:** publish task-oriented guides, a component catalog, and Markdown manpage-style reference pages.
2. **CLI discovery:** add searchable catalogs, `explain`, validation hints, and a guided `init`/`add` flow that writes reviewable files.
3. **Interactive board builder:** offer an optional terminal UI that composes `.espf` from a live catalog and shows pin conflicts before writing.
4. **Application recipes:** make `src/app.rs` discoverable through runnable, copyable behavior recipes and Rust API examples.

The recommendation is to start with the docs and a small set of noninteractive CLI discovery commands. Then add a TUI only after the catalog and capability metadata are good enough to power it. The TUI should generate ordinary `.espf` and `src/app.rs` files; those files remain the user's project and need no special runtime.

## Current friction

### 1. Users must invent the hardware specification

The introductory README has a minimal LED example, but a real project author must infer the shape and ordering of `# espforge`, `# peripherals`, `# devices`, and `# services`, then know which tags and attributes exist. `espforge examples` lists example names and titles, not the hardware inventory or wiring each example demonstrates. `espforge new` chooses an embedded example, chip, and output directory; it does not ask which board or sensor the user has.

The sensor question is especially difficult: “I have an SHT31 breakout” does not directly tell a new user to declare an I²C peripheral, wire SDA/SCL to supported GPIOs, bind `<sht31>` to `$i2c0`, choose an address, or understand which address is valid for their board/module.

### 2. Users must infer the hardware-to-Rust boundary

The `.espf` file declares the board resources and devices. `src/app.rs` contains user behavior in Rust. Generated code connects them through `Context` and `device!(ctx, id)`, but users have to learn this from examples or generated source. They need to understand:

- Which work belongs in `.espf` (pins, bus, device options, services).
- Which work belongs in `src/app.rs` (read a sensor, react to a button, update a display, periodic scheduling).
- How blocking and embassy runtimes change function shapes and awaits.
- How boot versus deferred initialization affects when a device becomes available.
- Which driver crate APIs are exposed and what methods/traits are callable.

An ESPHOME-style YAML component often includes enough configuration to express the behavior users want, while ESPForge intentionally uses Rust for application logic. That division is powerful for Rust users, but it means an example needs both a complete `.espf` and a complete, explained `app.rs` to be useful.

### 3. The reference is split across different kinds of documents

The README covers the first blink example, while external-driver docs explain KDL code generation for driver authors. Architecture journey docs explain internals and generated Rust. Those are useful, but a new user looking for “SHT31 on ESP32-C3, print temperature every second” has no single task page showing wiring, spec, app code, command, expected output, and troubleshooting.

There is no sensor/component catalog command, no `espforge help <sensor>` reference, and no documented catalog page mapping a physical module name to a tag, supported transport, wiring, and example.

### 4. Validation is a gate, not a guide

`espforge check` validates the spec, app logic, and overlay inputs, but it does not generate or compile the project. Its success message explicitly says that `build` verifies the generated project. `espforge build` eventually runs Cargo, but a Cargo error can be distant from the original question: “What belongs in `app.rs`?” or “Does this tag support embassy?”

Located diagnostics are a good foundation. The next UX step is to attach contextual next actions: show the accepted tags/options, point to a working example, and explain whether a failure requires a spec change, app code change, driver installation, or toolchain setup.

### 5. The CLI's existing wizard only scaffolds an example

`espforge new` interactively asks for example category, example, chip, and directory. This is helpful for discovering the example catalog, but it does not help users compose a new board from their actual parts. `examples` is grouped by category and prints names/titles, with no search/filter-by-part capability.

### 6. Driver commands are powerful but aimed at driver maintainers

`espforge driver add/list/update/remove/verify/migrate` supports local and fetched manifests, lockfiles, and validation. It does not answer the end-user discovery questions: “Is this sensor supported?”, “What is its tag?”, “What crate will this add?”, “Can I use it with my chip/runtime?”, or “Show me a working app.” Driver package plumbing is not a substitute for an end-user hardware catalog.

## Lessons from ESPHome

ESPHome is a useful reference for discoverability, not a requirement to copy its YAML execution model. Its official getting-started guide presents a clear lifecycle: create a configuration, add hardware, validate, install, and view logs. The same configuration works through the dashboard or CLI, and the CLI documents counterparts such as `wizard`, `config`, `run`, `logs`, and `clean`. Its component docs and configuration examples let users search for a component and copy a small, complete snippet. Reusable packages and substitutions address repetition across devices.

Useful lessons for ESPForge:

- Keep one canonical project description usable from CLI, editor, and any UI.
- Put examples beside the component reference, not only in a separate example gallery.
- Use a predictable lifecycle vocabulary and make each step's result clear.
- Let users start from hardware they own, not only from a generic project example.
- Make copied examples complete enough to validate/build and explain what is safe to edit.
- Make reuse easy for common board wiring and repeated application patterns.

Important difference: ESPForge users must write some Rust for application behavior. The UX should guide them to the right Rust recipe and APIs instead of implying that adding a sensor tag alone creates a complete application.

References: [ESPHome Getting Started](https://esphome.io/install/getting-started/), [ESPHome substitutions](https://esphome.io/components/substitutions/), [ESPHome packages](https://esphome.io/components/packages/).

## UX principles

1. **Start with user intent or physical hardware.** “Read a temperature sensor” and “SHT31” should both lead to a working path.
2. **Show the exact files.** Users should see what will be written to `.espf` and `src/app.rs` before accepting a scaffold.
3. **Always give a working next step.** Every reference page ends with a check/build command and a complete example.
4. **Distinguish generated and user-owned files.** Explain that build output is disposable and app/spec sources are the project inputs.
5. **Prefer inspectable text artifacts.** A wizard composes normal files; it does not hide configuration in an opaque database.
6. **Use existing capability data.** Catalog support status should come from driver manifests/schema metadata where possible, not from a hand-maintained second truth.
7. **Teach in layers.** A quick start for beginners, reference for exact details, and internals for driver authors.
8. **Do not overpromise.** State chip, runtime, transport, crate, and app-logic requirements explicitly.

## Orthogonal approaches

These approaches can ship independently. They reinforce one another, but no single one is a prerequisite for the others except that a TUI benefits from a structured catalog.

### Approach A — Task-oriented docs and runnable recipes

Add a “Build a project” guide and a series of end-to-end recipes. Each recipe includes:

- Parts and wiring table, including voltage/level notes where material.
- Complete `.espf` file.
- Complete `src/app.rs` file.
- One exact command sequence (`check`, `build`, and, where appropriate, flashing/log viewing instructions).
- Expected build/runtime behavior and common errors.
- A short explanation of what the user edits and what generated files they should leave alone.
- Links to the element reference and the Rust driver API.

Start with blink, button-to-LED, I²C scanner, SHT31 periodic reading, SSD1306 sensor display, and async button/sensor examples. Add a recipe only when it builds and its wiring is reviewed.

**Advantages:** lowest engineering risk, improves all users, and produces content reusable by CLI hints and TUI.  
**Limitations:** users still have to discover and adapt snippets manually; docs can drift unless examples are validated.

### Approach B — Markdown manpage-style reference

Create a structured Markdown reference that is readable on GitHub, in editors, and in a future `espforge help` command. Example layout:

```text
docs/reference/
  index.md
  concepts/
    project-files.md
    espf-sections.md
    context-and-app-rs.md
    blocking-and-embassy.md
  elements/
    peripherals/i2c.md
    peripherals/spi.md
    devices/sht31.md
    devices/ssd1306.md
  recipes/
    temperature-to-serial.md
    button-to-display.md
  troubleshooting/
    first-build.md
    device-not-found.md
```

Each element page should follow a stable manpage contract:

```markdown
---
title: sht31 — I²C temperature and humidity sensor
section: devices
tag: sht31
---

# NAME

sht31 — configure an SHT31 temperature/humidity sensor

# SYNOPSIS

<sht31 id="air" bus="$i2c0" address="0x44" />

# DESCRIPTION
...

# ATTRIBUTES
...

# WIRING
...

# EXAMPLES
## Minimal spec
...
## Read values in app.rs
...

# RUNTIME SUPPORT
...

# SEE ALSO
...
```

The page needs both a human-facing concise reference and complete runnable samples. Metadata can later be generated from `Schema`/manifest contents (tag, summary, attributes, defaults, examples, see-also) while editorial sections provide wiring and Rust usage.

**CLI integration options:**

- `espforge help sht31` resolves a tag and renders its reference page.
- `espforge help devices/sht31` and `espforge help app` support explicit sections.
- `espforge help --list` lists topics and searches titles/tags.
- `espforge --help` remains a command synopsis, not a replacement for the guide.
- Optionally export generated roff pages using `clap_mangen` for traditional `man espforge`; keep Markdown as the authored source.

**Advantages:** durable, searchable, editor-friendly and suitable for static site generation.  
**Limitations:** demands content maintenance and a renderer/topic index if served in the CLI.

### Approach C — Improve the CLI's discovery and explanation commands

Add small commands that make current capabilities visible without requiring an interactive terminal:

```sh
espforge catalog
espforge catalog search temperature
espforge catalog show sht31
espforge explain my-project.espf
espforge explain --example 06.Displays/ssd1306_wifi
espforge doctor --project .
```

Suggested behavior:

- `catalog` filters by keyword, category, transport, chip, and runtime. Results include tag, short summary, and a command to open details.
- `catalog show <tag>` displays synopsis, attributes/defaults, supported transports/runtime, required crates, wiring guidance link, and runnable examples.
- `explain` parses a project and prints a human-readable board map: chip/runtime, peripherals and pins, devices and bus bindings, services, app entry points, and resolved driver sources.
- `check --explain` optionally prints “why” context for inferred defaults or resolved bus modes.
- A diagnostic footer suggests the closest actionable command or docs topic, for example `espforge help devices/sht31`.
- `doctor --project` separates host toolchain readiness from project/schema correctness and offers focused fixes.

**Advantages:** helps both interactive and scripted users; metadata and command output are easy to test.  
**Limitations:** text search can still overwhelm beginners, and catalog accuracy depends on capability metadata.

### Approach D — Guided `init` / `add` wizard and terminal board builder

Extend the current `new` wizard or add a separate `espforge init` flow. Ask for project intent, chip, runtime, then hardware. Let users search the catalog, pick a device, select bus/pins/address/options, and add another part. Show a wiring checklist and a live summary. Before writing, preview the resulting `.espf` and `src/app.rs` recipe, then ask for the destination.

Possible compact CLI flow:

```text
? What do you want to build?  Read and display temperature
? Choose a sensor:            SHT31 (I²C)
? I²C controller:             i2c0
? SDA pin:                    GPIO6
? SCL pin:                    GPIO7
? Sensor address:             0x44
? Output:                     serial log
? Project name/directory:     room_monitor

Preview: room_monitor.espf, src/app.rs
Build now? [y/N]
```

In v1, a TUI should only select schema-known elements and validated attributes. It should not synthesize arbitrary Rust. For app behavior, offer named recipes whose Rust source is reviewed and compiled, with an “open file and edit” handoff.

**Advantages:** turns discoverability into an active workflow and catches pin conflicts before file creation.  
**Limitations:** the option space grows quickly; a TUI cannot replace docs or teach Rust by itself.

### Approach E — Hardware gallery and community catalog

Publish a searchable web catalog that maps common product/module names and aliases to ESPForge tags, transport, wiring, tested chips, runtime support, driver crate, and a complete app recipe. Support package-owned pages and versioned compatibility data. Start curated and local/offline friendly; avoid an unmoderated registry until maintenance and trust requirements are clear.

**Advantages:** best fit for “I bought this board/sensor; what now?” and can collect multiple module aliases.  
**Limitations:** hosting, review, version compatibility, and stale hardware listings require ownership.

### Approach F — Editor support

Provide VS Code/Language Server features for `.espf`: tag and attribute completion, inline docs, hover showing defaults and valid values, diagnostics from the same checker, and “insert example” actions. Add a Rust `app.rs` helper page and code snippets for `device!`, deferred `init`, and blocking/async loops.

**Advantages:** keeps guidance next to the file being edited and reduces spelling errors.  
**Limitations:** only helps users in supported editors; should consume the same schema/catalog as CLI and docs.

## `src/app.rs` needs a first-class guide

The app guide should answer concrete questions, not just describe the generated `Context` type.

### Minimum beginner guide

1. What ESPForge generates and which files are user-owned.
2. The `setup(ctx)` and `forever(ctx)` contract for blocking projects.
3. The embassy task contract for async projects and how `device!` obtains a device guard.
4. How device IDs in `.espf` become Rust fields.
5. How to read one sensor value and log it.
6. How to read a button and control a display/LED.
7. How deferred devices are initialized and why setup ordering matters.
8. How to add a Rust crate or driver and how to inspect generated code when debugging.
9. How to use check/build feedback and where compilation failures come from.

### Suggested recipe names

- `app/hello-log`
- `app/blink-periodic`
- `app/button-to-led`
- `app/read-i2c-sensor`
- `app/read-sensor-and-display`
- `app/async-periodic-task`
- `app/deferred-device-init`
- `app/uart-sensor`

Each recipe should show the whole `app.rs`, not only a single line, and state which spec IDs it expects. Where API usage differs across drivers, link to the crate docs and include the exact trait import.

### Optional app templates

Allow a recipe to be selected during `espforge new` or `espforge add app-recipe`, while keeping `src/app.rs` a normal editable Rust file. Store the recipe catalog as examples or templates in the repository and test each generated project. Do not introduce a second declarative language for application logic.

## Proposed staged delivery

### Phase 1 — Make the existing path understandable

- Add the project files and app.rs beginner guide.
- Add end-to-end task recipes with paired `.espf` and `app.rs`.
- Improve `espforge examples` with search, summary, parts/tags, supported chip/runtime, and detail links.
- Add Markdown reference structure and begin generating element metadata from the schema.

### Phase 2 — Make CLI output answer “what next?”

- Implement `espforge catalog` / `catalog show` and topic lookup (`espforge help <topic>`).
- Add `espforge explain` to print the resolved project graph and app contract.
- Enrich diagnostics with relevant docs/example links and action commands.
- Make `check`/`build` messages precise about what each command verified.

### Phase 3 — Guide project composition

- Add a guided `init` or `add device` flow using catalog metadata.
- Preview files before writing and detect pin/address conflicts as selections change.
- Generate a paired app recipe when the user chooses an end-to-end task.
- Keep noninteractive flags for all choices so the workflow remains automatable.

### Phase 4 — Add richer surfaces only when justified

- Build an optional TUI with search, board graph, pin map, and preview.
- Add editor/LSP support using the same schema and diagnostics.
- Evaluate a web hardware gallery or community catalog with versioning and review policy.

## GitHub Projects work items

Use the following titles as issue/card names. Suggested fields: `Status`, `Milestone`, `Priority`, `Area`, and `Estimate`.

| Work item | Milestone | Priority | Area | Estimate |
|---|---|---:|---|---:|
| **[Docs] Write the first-project guide from `.espf` to `app.rs`** | UX 1 — First successful project | P0 | docs | M |
| **[Docs] Add paired spec/app recipes for common tasks** | UX 1 — First successful project | P0 | docs, examples | L |
| **[Docs] Establish Markdown manpage format for element references** | UX 1 — First successful project | P1 | docs | M |
| **[Examples] Add metadata for parts, chips, transports, and runtime** | UX 1 — First successful project | P1 | examples, schema | M |
| **[CLI] Add searchable `espforge catalog` and `catalog show`** | UX 2 — Discover hardware | P1 | CLI | M |
| **[CLI] Add `espforge explain` project graph output** | UX 2 — Discover hardware | P1 | CLI, elements | M |
| **[Diagnostics] Suggest relevant docs and examples from validation errors** | UX 2 — Discover hardware | P1 | CLI, diagnostics | M |
| **[CLI] Add guided device selection and file preview** | UX 3 — Compose a board | P1 | CLI, schema | L |
| **[TUI] Prototype searchable board builder over schema metadata** | UX 4 — Rich editing | P2 | CLI, TUI | XL |
| **[Editor] Publish `.espf` language support and Rust app snippets** | UX 4 — Rich editing | P2 | editor, schema | XL |
| **[Catalog] Define review and compatibility policy for community drivers** | UX 4 — Rich editing | P2 | docs, drivers | M |

### Issue: [Docs] Write the first-project guide from `.espf` to `app.rs`

**User story:** As a first-time ESPForge user, I want to know what goes in each project file and how app behavior connects to configured devices, so I can adapt a working example without reverse engineering generated code.

**Acceptance criteria:**

- Shows a complete `.espf` and `src/app.rs` for at least blink and sensor-to-log.
- Explains device IDs, `Context`, runtime mode, blocking vs embassy, and deferred init.
- Names which files are source inputs and which are disposable generated output.
- Links to `check`, `build`, `catalog`, and troubleshooting guidance.
- Every command and example is validated against the current CLI.

### Issue: [Docs] Establish Markdown manpage format for element references

**User story:** As a user who knows a component name, I want a concise searchable reference and complete example without leaving the terminal or repo.

**Acceptance criteria:**

- Define metadata/frontmatter and required sections (`NAME`, `SYNOPSIS`, `DESCRIPTION`, `ATTRIBUTES`, `WIRING`, `EXAMPLES`, `RUNTIME SUPPORT`, `SEE ALSO`).
- The schema supplies tag, summary, attributes, defaults, choices, examples, and related tags where possible.
- Editorial wiring, Rust app usage, caveats, and tested-chip information can be maintained alongside generated metadata.
- `espforge help <tag>` can render the page or provides a clear fallback link when docs are not bundled.

### Issue: [CLI] Add searchable `espforge catalog`

**Acceptance criteria:**

- Supports terminal-friendly filtering by text, tag, category, transport, chip, and runtime.
- Supports stable machine-readable output such as `--json`.
- `catalog show <tag>` prints a spec snippet, valid attributes, summary, and reference/example location.
- Unknown searches suggest close matches and do not dump a long undifferentiated list.
- Catalog metadata has a single source of truth or a consistency check against manifests/schema.

### Issue: [CLI] Add `espforge explain`

**Acceptance criteria:**

- Prints the resolved chip/runtime and a table/diagram of peripherals, pins, devices, and services.
- Shows each device's resolved bus, address or CS, init mode, driver crate/source, and supported runtime.
- Lists the app hooks and fields users can reference in `Context`.
- Does not write generated output; offers `espforge build` as the next command.

### Issue: [CLI] Add guided device selection and file preview

**Acceptance criteria:**

- Supports both TTY prompts and equivalent noninteractive flags.
- Searches the same catalog as `espforge catalog`.
- Collects only schema-valid attributes and offers board-valid pin/peripheral choices.
- Shows the exact `.espf` diff and optional app recipe before writing.
- Runs project validation after writing and reports file paths and next actions.
- Never silently overwrites user-authored source files.

## Metrics and feedback

Track whether the UX work is reducing the first-success gap:

- Time and number of commands from `espforge new` to first successful `build`.
- Share of guide recipes that compile in CI for their declared chip/runtime.
- Number of users who find a driver through catalog search versus existing example names.
- Common `check`/`build` diagnostics that lead to repeat attempts without a fix.
- Qualitative interviews: can a new user explain what belongs in `.espf` versus `app.rs` after using the guide?

Do not optimize around command count alone. A command that scaffolds an opaque project is worse than a few commands that teach and leave clear, editable files.

## Current repo touchpoints

- `README.md` — top-level promise, basic blink example, current CLI workflow.
- `docs/001-basics.md` — basics documentation; candidate home for first-project walkthrough.
- `docs/002-external-driver-manifest.md` — advanced driver-author reference; not an end-user sensor guide.
- `espforge/src/main.rs` — command list and Clap help.
- `espforge/src/commands/examples.rs` and `new.rs` — current catalog output and scaffold wizard.
- `espforge/src/commands/check.rs`, `generate.rs`, `build.rs`, and `report.rs` — command lifecycle and user feedback.
- `espforge-elements/src/schema.rs` and composed schema manifests — source for tag/attribute metadata.
- `espforge-examples` — existing embedded examples and potential source for runnable recipes.
- `architecture/journey-*.md` — valuable internals walkthroughs; keep them distinct from user task guides.

## Suggested first release slice

For the highest impact with manageable scope, ship these together:

1. A first-project guide explaining `.espf` and `app.rs`.
2. Three paired runnable recipes: button-to-LED, I²C sensor-to-log, and sensor-to-display.
3. `espforge catalog search/show` backed by schema and example metadata.
4. Markdown reference pages with `espforge help <tag>` lookup.
5. Better `check` errors that name a matching docs page and example.

This creates the content and metadata foundation for a later board-builder TUI, while immediately helping users who prefer an editor, terminal commands, or static docs.
