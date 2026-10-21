# Plan: `espforge espf`, the ESPF discovery command

Status: draft 3 for discussion. Nothing here is implemented yet.

Changes since draft 2: a bare section acts like `help` and lists its leafs as subcommands (one per peripheral, component, and so on); the root gets an `overview` subcommand that explains the ESPF format; the root list is now sections plus `overview`.

Changes since draft 1: a leaf lists its two subcommands (`help`, `examples`) instead of showing a view; `help` is the man page and `examples` is the cheat.sh style view; `man` is gone; `--json` is gone.

## 1. Goal

A user who has just installed espforge and does not know what an ESPF file can contain should be able to find out from the terminal, in a few keystrokes, without leaving it.

Concretely they should be able to:

1. Learn what an ESPF file is and how it is laid out (`overview`).
2. See the five sections and what each is for.
3. See the elements (leafs) available in a section.
4. See, for one leaf, what they can ask about it.
5. Read a man page for it: description, attributes (type, required or optional, default), what it connects to.
6. Copy working examples of it, and find their way to a full example project.

### Non-goals

- Editing ESPF files (no `add` / `set` / `rm`). It can come later on top of the same catalog.
- A full-screen TUI (ratatui). Discovery is text-first.
- JSON output. Output is for people; no machine-readable format is promised.
- Replacing `check` or `build` validation in this feature. The catalog may drive validation later, but phase 1 must not change validation behaviour.

## 2. Decisions so far

| # | Decision | Source |
|---|----------|--------|
| D1 | The command is `espforge espf`, followed by a section, then a leaf in that section | user |
| D2 | There is no `list` or `show` subcommand; the catalog path itself is the interface | user |
| D3 | `espforge espf <section> <leaf>` lists that leaf's subcommands, and only those | user |
| D4 | The leaf subcommands are exactly `help` and `examples` | user |
| D5 | `help` is the man page | user |
| D6 | `examples` is the cheat.sh style view | user |
| D7 | No `--json` | user |
| D8 | `espforge espf <section>` acts like help: it lists the section's leafs as subcommands, one per element | user |
| D9 | `espforge espf overview` explains the ESPF format | user |
| D10 | One catalog file (`elements.kdl`) is the single source of truth for the element views | proposed |

## 3. Command surface

### 3.1 Grammar

```
espforge espf [overview | SECTION [LEAF [help | examples]]] [FLAGS]
```

```
espforge espf                          sections and overview, as subcommands
espforge espf overview                 the ESPF format in one page
espforge espf peripherals              the section's description and its leafs, as subcommands
espforge espf peripherals help         same as above
espforge espf peripherals i2c          the subcommands of i2c: help, examples
espforge espf peripherals i2c help     man page for i2c
espforge espf peripherals i2c examples cheat sheet for i2c
espforge espf i2c                      shorthand: a unique leaf name resolves without its section
```

Alias: `ref` (`espforge ref peripherals i2c`). `espf` next to `espforge` reads oddly to new users, so the friendlier name is worth having.

### 3.2 The uniform rule

At every level, the bare command lists its children as complete, copyable commands (like clap's subcommand list). Only the leaf is different: its `help` is a real page, not a list.

| Path | bare | `help` | `examples` |
|------|------|--------|------------|
| root | sections, then `overview` | same as bare | error: needs a leaf |
| `overview` | the format overview | n/a (no children) | error |
| section | description, then its leafs | same as bare | error: needs a leaf |
| leaf | its subcommands | man page for the leaf | cheat sheet for the leaf |

So `help` is only meaningfully different from the bare command at the leaf. Elsewhere it is kept as an alias because trailing `help` is already how espforge users ask for help.

### 3.3 Resolution rules

Implemented once in `catalog.rs` and shared by every view.

1. If the last argument is `help` or `examples`, strip it and remember it as the view. This happens before any lookup, so no leaf can be named `help` or `examples`.
2. No remaining arguments: root.
3. One argument:
   1. `overview`: the overview page;
   2. matches a section: that section;
   3. else matches exactly one leaf tag across all sections: that leaf;
   4. else matches several leafs: error listing the `section leaf` forms that work;
   5. else: error with "did you mean" suggestions and the list of sections.
4. Two arguments: `SECTION LEAF`. An unknown section or unknown leaf gives a "did you mean" error. A leaf that exists in a different section gets a pointed hint ("`i2c` is in `peripherals`").
5. `overview` takes no further arguments; more than two arguments is an error.
6. The view is then checked against the level (table in 3.2); `examples` anywhere but a leaf is an error that says so.
7. Reserved names, enforced by a catalog test: no section or leaf may be called `help`, `examples` or `overview`, and no leaf may share a name with a section.

### 3.4 Flags

| Flag | Meaning | Phase |
|------|---------|-------|
| `--chip <chip>` | Hide leafs that do not apply to this chip | 3 |
| `--no-pager` | Never pipe long output (`overview`, leaf `help`) through a pager | 2 |
| `--roff` | With leaf `help`, emit real roff for packagers | 4 (optional) |
| `-C <dir>` | Already global; unaffected | n/a |

Exit codes follow the existing convention: 0 success, 1 user error (unknown name; message already printed), 2 internal failure.

## 4. Output design

### 4.1 Root

Aligned columns on a terminal, tab-separated when piped. Each line is a complete command.

```
$ espforge espf
espforge espf espforge      project, chip, runtime, console, options for the build
espforge espf peripherals   the chip's hardware blocks you use: gpio, i2c, spi, uart
espforge espf components    how a peripheral is used: an address on a bus, a chip-select
espforge espf devices       drivers for specific parts: displays, sensors
espforge espf services      software services: wifi, ntp, http
espforge espf overview      what an ESPF file is and how it is laid out
```

### 4.2 Section

A short description paragraph (the section's `description`, from the catalog), then one command per leaf.

```
$ espforge espf peripherals
The chip's hardware blocks that your project uses. Each one gets an id that
components refer to with $id.

espforge espf peripherals gpio    a single GPIO pin
espforge espf peripherals i2c     I2C bus controller
espforge espf peripherals spi     SPI bus controller
espforge espf peripherals uart    UART port
```

A leaf that cannot be built yet carries a marker after its summary (see 6.5), for example `planned`.

### 4.3 Leaf subcommand list

```
$ espforge espf peripherals i2c
espforge espf peripherals i2c help        man page: description, attributes, see also
espforge espf peripherals i2c examples    copy-ready snippets and example projects
```

Piped, it is `command<TAB>description`. Whether a one-line title (`i2c  I2C bus controller`) goes above these is open (11.4).

### 4.4 `examples`: cheat sheet

Examples only, no reference tables. Everything printed is valid ESPF, so it can be pasted straight into a file. ESPF has no standalone comment line (a line starting with `#` is a section header, and a standalone `//` is junk), so labels are written as the trailing `//` comment the grammar does allow, on the first element line of each snippet.

```
$ espforge espf peripherals i2c examples
<i2c id="i2c0" sda="6" scl="7" />                          // minimal

<i2c id="i2c0" sda="6" scl="7" frequency="400kHz" />       // with a bus speed

# peripherals
  <i2c id="i2c0" sda="6" scl="7" />                        // referenced by a component
# components
  <i2c_component id="temp_bus" i2c="$i2c0" address="0x48" />

used in
  04.Communication/i2c_scanner
  04.Communication/i2c_example
  06.Displays/ssd1306_example

try
  espforge examples get 04.Communication/i2c_scanner
```

- Snippets come from the catalog (a leaf can have several, each with a title).
- `used in` and `try` are computed (see 6.4), not written by hand.
- Never paged: it is short.

### 4.5 Leaf `help`: man page

```
NAME
    i2c - I2C bus controller

SYNOPSIS
    <i2c id="..." sda="PIN" scl="PIN" [frequency="FREQ"] />

DESCRIPTION
    Declares an I2C controller. Reference it from a component with i2c="$id".

ATTRIBUTES
    id          (required, ident) Name other elements use to refer to this one ($id).
    sda, scl    (required, pin) GPIO numbers; must be valid for the chosen chip.
    frequency   (optional, freq, default 100kHz) Bus speed, e.g. 100kHz or 400kHz.

REFERENCED BY
    components/i2c_component   via i2c="$id"

SEE ALSO
    espforge espf components i2c_component
    espforge espf peripherals i2c examples
```

Blocks with no data (no description, no `see_also`) are skipped, so a leaf can ship with only the generated parts (name, synopsis, attributes, referenced by) and gain prose later.

On a terminal this goes through `$PAGER` (default `less -R`), like `git help`. Piped, it prints plain. A test lists leafs that still lack prose, as a to-do, not a failure.

### 4.6 `overview`: the format in one page

A single page for someone who has never seen an ESPF file. Man-page style, paged on a terminal. It covers, in this order:

1. What an ESPF file is: a description of a firmware project that espforge turns into a Rust project.
2. The five mandatory sections (empty is allowed) and the flow between them: `peripherals` (hardware) are used by `components`, which are used by `devices`; `services` are software; `espforge` is the project itself.
3. Syntax: one self-closing element per line, `<tag attr="value" />`; every value is a double-quoted string and cannot span lines; a trailing `// comment` is allowed after an element; a header comment block (lines of `=`) is allowed only at the top of the file.
4. References: an `id` names an element and `$id` refers to it from another; ids are global across sections.
5. Dotted keys (`network1.ssid="home"`) and lists (`servers+="a" servers+="b"`, with `+=` on every entry including the first).
6. A complete minimal file.
7. Where to go next: `espforge espf peripherals`, `espforge examples`, `espforge new`.

Sketch:

```
$ espforge espf overview
NAME
    ESPF - the file format espforge builds firmware from

DESCRIPTION
    An ESPF file lists the hardware your project uses and how it is wired,
    and espforge generates a Rust firmware project from it. It has five
    sections, in any order, and every one must be present (it may be empty).

      espforge      the project: name, chip, runtime, console, options
      peripherals   hardware blocks of the chip: gpio, i2c, spi, uart
      components    how a peripheral is used (an address, a direction)
      devices       drivers for specific parts built on a component
      services      software services: wifi, ntp, http

SYNTAX
    One element per line, written as a self-closing tag:

      <tag id="name" attribute="value" />

    ...

EXAMPLE
      # espforge
        <project name="blink" />
        <chip type="esp32c3" />
        <runtime type="blocking" />
      # peripherals
        <gpio id="gpio4" pin="4" />
      # components
        <gpio_component id="blue_led" gpio="$gpio4" direction="output" />
      # devices
      # services

NEXT
    espforge espf peripherals     browse what you can declare
    espforge examples             copy a complete example project
```

The text lives in its own file (see 5) so it can be edited as prose, not as KDL.

### 4.7 The `espforge` section

This section is not a list of ordinary element definitions, so its leafs are partly generated:

- `espforge espf espforge chip`: values come from `chips::offered_chips()`, with whether each has USB-Serial-JTAG (`chips::has_usb_serial_jtag`).
- `espforge espf espforge runtime`: `blocking`, `embassy`.
- `espforge espf espforge console`: `uart`, `jtag-serial`, with the chip restriction.
- `espforge espf espforge options`: generated from `embedded_schema()` (every option, kind, default, `requires`, compatible chips, whether derived).

The catalog describes these leafs' shape; their values are read from the existing sources of truth so they cannot drift. They get `help` and `examples` like every other leaf.

## 5. Architecture

```
espforge-espgenerate-template/src/elements.kdl    new: the element catalog (embedded like options.kdl)
espforge-espgenerate-template/src/overview.txt    new: the prose of `overview` (embedded, plain text)
espforge-options/src/catalog.rs                   new: types, loader, resolve(), lookups; no printing
espforge/src/render.rs                            new: render_children, render_examples, render_man,
                                                       render_overview
espforge/src/commands/espf.rs                     new: argument handling, trailing-word strip, paging
espforge/src/main.rs                              changed: Espf command, expand_help bypass
```

Layering follows `cli.md`: catalog = data and lookup (Layer 0), render = pure functions returning `String`, command = I/O.

`render_children` is the one function behind the root, section and leaf lists: it takes a list of (command, description) pairs and prints them aligned (terminal) or tab-separated (piped). That is what the "uniform rule" in 3.2 buys.

Where the loader lives is open (11.1). The default proposal is `espforge-options`, next to `schema.rs`, because it already depends on `kdl`, `espf` diagnostics and the embedded template crate.

### 5.1 Data model (sketch)

```rust
pub struct Catalog {
    pub sections: Vec<SectionDef>,
    pub overview: &'static str,      // overview.txt
}

pub struct SectionDef {
    pub name: String,                // "peripherals"
    pub summary: String,             // one line, shown in the root list
    pub description: String,         // paragraph shown above the leaf list
    pub leafs: Vec<ElementDef>,
}

pub struct ElementDef {
    pub tag: String,
    pub section: String,
    pub summary: String,
    pub description: Option<String>,
    pub status: Status,              // Implemented | Planned
    pub attrs: Vec<AttrDef>,
    pub examples: Vec<Example>,      // at least one
    pub see_also: Vec<String>,       // "section/tag"
    pub compat: Compat,              // reuse schema::Compat
    pub span: Range<usize>,          // span in elements.kdl, for loader diagnostics
}

pub struct Example {
    pub title: String,               // printed as the trailing `// title`
    pub fragment: String,            // one or more lines of valid ESPF
}

pub struct AttrDef {
    pub name: String,                // dotted allowed: "network1.ssid"
    pub kind: AttrKind,
    pub required: bool,
    pub default: Option<String>,
    pub help: Option<String>,
}

pub enum AttrKind {
    Ident, Str, Pin, Freq, Hex, U32, Bool,
    Choice(Vec<String>),
    Ref { to: Vec<String> },         // `$id` reference to an element of these tags
    List(Box<AttrKind>),             // written with `+=`
}

pub enum Status { Implemented, Planned }

pub enum View { Default, Help, Examples }

pub enum Match<'a> {
    Root(&'a Catalog),
    Overview(&'a Catalog),
    Section(&'a SectionDef),
    Leaf(&'a ElementDef),
}
```

`Compat` and the loader's error style (`EspfError` with a span, rendered by `espf::diag`) are reused from `schema.rs`, so a broken `elements.kdl` produces the same snippet diagnostics as a broken `options.kdl`.

## 6. The catalog: `elements.kdl`

### 6.1 Format (KDL v2, same conventions as `options.kdl`)

```kdl
section "peripherals" {
    summary "The chip's hardware blocks you use"
    description "The chip's hardware blocks that your project uses. Each one gets an id that components refer to with $id."
}

element "i2c" section="peripherals" {
    summary "I2C bus controller"
    description "Declares an I2C controller. Reference it from a component with i2c=\"$id\"."
    attr "id" kind="ident" required=#true
    attr "sda" kind="pin" required=#true {
        help "GPIO number; must be valid for the chosen chip."
    }
    attr "scl" kind="pin" required=#true
    attr "frequency" kind="freq" default="100kHz"
    example "minimal" `<i2c id="i2c0" sda="6" scl="7" />`
    example "with a bus speed" `<i2c id="i2c0" sda="6" scl="7" frequency="400kHz" />`
    see_also "components/i2c_component"
}
```

| Item | Required | Notes |
|------|----------|-------|
| `section` node: `summary`, `description` | yes | `summary` is the root-list line, `description` the paragraph above a section's leafs |
| `element` `section` property | yes | must name a defined section |
| `summary` | yes | one line; shown in lists |
| `example "title" \`fragment\`` | yes, one or more | the fragment is valid ESPF; parsed by tests |
| `attr` | no | `kind`, `required`, `default`, child `help` |
| `description`, `see_also` | no | man page prose; skipped when absent |
| `status` property | no | `planned` marks not-yet-buildable leafs; default implemented |
| `compatible { chip ... }` | no | reuse the existing `Compat` shape |

An example fragment may span several lines and include `# section` headers (as in the components snippet in 4.4). The tests parse it as a fragment: no junk lines, every tag exists in the catalog, every attribute is declared, required attributes are present. Missing sections in a fragment are not an error.

### 6.2 Attribute kinds

| Kind | Example | Notes |
|------|---------|-------|
| `ident` | `i2c0` | letters, digits, `_`, `-` |
| `str` | `"home"` | free text |
| `pin` | `6` | a GPIO number; chip-aware validity is a later phase (11.8) |
| `freq` | `100kHz` | number plus Hz, kHz or MHz |
| `hex` | `0x48` | I2C addresses |
| `u32` | `9600` | baud rate and similar |
| `bool` | `true` | `"true"` or `"false"` |
| `choice` | `none`, `even`, `odd` | `choices` child |
| `ref` | `$i2c0` | `to=` names the tags it may point at |
| `list` | `servers+=` | written with `+=` on every entry |

Dotted attributes (for services like `wifi`) are declared with their dotted name, for example `attr "network1.ssid"`. Repeating numbered groups are open (11.9).

### 6.3 Existing sources of truth the catalog must agree with

Today "what exists" is spread over several places that do not agree:

| Place | What it knows |
|-------|---------------|
| `espf/src/context.rs` | peripheral tags `i2c`, `spi`, `uart`, everything else treated as gpio; component link attributes `i2c`, `spi`, `gpio`, `uart`; devices need a `component` attribute (any key containing `component`) |
| `espforge-bindings/src/components/*` | wrappers for `gpio_component`, `i2c_component`, `spi_component`, `uart_component` with their own required attributes |
| `espforge-bindings` driver registry (generated by `build.rs`) | device drivers `status_led`, `ili9341`, `tmp102` |
| `espforge-options` | the `# espforge` section: chips, runtimes, consoles, options |
| embedded examples | tags used in practice, several of which nothing implements |

The catalog does not replace these in phase 1. It is a new, checked description; the drift tests (section 8) keep it honest.

### 6.4 Computed information (never hand-written)

- `REFERENCED BY` in the man page: for each `ref` attribute in the catalog, the reverse index (`i2c` is referenced by `i2c_component`).
- `used in` in the cheat sheet: parse every embedded example with `parse_espf` and index tags to `category/name`.
- `try`: the first entry of `used in`, formatted as `espforge examples get <category>/<name>`.

### 6.5 Status marker

Some examples use tags nothing implements, and `pipeline::buildable` refuses any project with a non-empty `peripherals`, `components`, `devices` or `services` section. So "exists in the catalog" and "can be built today" differ. Two levels, to confirm in 11.2:

- `implemented`: parses, validates and builds today
- `planned`: documented and used in examples, but not buildable yet; listed with a marker

### 6.6 The overview text

`overview.txt` is plain text, kept out of KDL so it reads and edits as prose. It contains one complete sample ESPF file. A test extracts the sample and runs it through the same validation `check` uses (everything except the buildable check), so the overview can never show a file that espforge rejects.

## 7. Integration with the existing CLI

### 7.1 `main.rs`

- New command `Espf` with `alias = "ref"`, taking a trailing var-arg `path: Vec<String>` and the flags from 3.4. It does not use clap subcommands per section or leaf; the catalog is data, not clap structure (alternative in 11.3).
- `expand_help` rewrites a `help` that directly follows a known command name into `--help`. For `espf` the trailing `help` must be consumed by our own parser, so the rewrite must not apply to anything after `espf` or `ref`. Rule: if the first non-flag argument is `espf` or `ref`, `expand_help` returns the arguments untouched. This also protects against a future leaf whose tag equals a name in `COMMAND_NAMES` (for example a device called `doctor`).
- `espforge espf --help` still shows clap's own help describing the grammar. Whether `--help` after a path means the leaf's man page is open (11.6).
- Add `espf` and `ref` to the command-existence test.

### 7.2 Diagnostics: hints that point into the catalog

Phase 3. `EspfError` gets an optional `help: Option<String>`, and `render_errors` prints it as a footer. Errors from `context.rs`, the parser and options resolution then end with lines like:

```
  help: run `espforge espf peripherals i2c help` to see how this element is declared
```

For format errors (junk lines, missing sections) the hint is `espforge espf overview`. This puts discovery where users are already stuck. It is a separate phase so it cannot delay the browser.

### 7.3 `examples` integration

After `examples get` and in the interactive `examples` flow, print one line per tag used in the example just copied: `espforge espf <tag> examples`. No extra prompts. Note the two different things called examples: the top-level `espforge examples` (whole projects) and the leaf subcommand `espf ... examples` (snippets). The `try` line connects them (see 11.7).

## 8. Drift tests (the catalog as a checked contract)

All run in `cargo test`, no terminal needed.

1. Every example fragment in `elements.kdl` parses with no junk lines.
2. Every element in every fragment has a tag in the catalog, uses only declared attributes, and includes the required ones.
3. Every `ref` attribute's `to` names a real tag.
4. Every `see_also` target exists.
5. No section or leaf is named `help`, `examples` or `overview`, and no leaf shares a name with a section.
6. Tags are unique, or a duplicate is explicitly allowed (see 11.10).
7. Every tag used by any embedded example exists in the catalog (implemented or planned). This will immediately report the `led pin=` versus `led gpio=` inconsistency and similar.
8. Every driver in `espforge-bindings`'s registry has a `devices` entry.
9. Every component wrapper in `espforge-bindings/components` has a `components` entry whose required attributes match the wrapper's.
10. The `espforge` section's values (chips, runtimes, consoles) come from the existing functions; a test asserts the catalog adds no hardcoded list.
11. The sample file in `overview.txt` passes the same validation as `check` (without the buildable check).
12. Snapshot tests of `render_children` (root, one section, one leaf), `render_examples` and `render_man` for two representative leafs (one simple, one with refs and a list).
13. Resolution tests for every rule in 3.3, including the wrong-section hint, the ambiguous tag, `overview`, and `examples` on a section.
14. `main.rs` test: `espf peripherals i2c help` is not rewritten to `--help`.

## 9. Phases

Each phase leaves the tree compiling with tests green and is useful by itself.

### Phase 1: the catalog and browsing

- `elements.kdl` with all sections, and leafs for what exists today (`gpio`, `i2c`, `spi`, `uart`, the four `*_component` tags, `status_led`, `ili9341`, `tmp102`) plus the `espforge` section's elements.
- `overview.txt` with a first version of the page.
- `catalog.rs` loader with spanned diagnostics, `resolve()`.
- `render_children`, `render_examples`, a basic `render_man` (generated parts only: name, synopsis, attributes, referenced by), `render_overview`.
- `espf` command and `ref` alias, trailing-word handling, `expand_help` bypass.
- Drift tests 1-6, 8, 9, 11, 13, 14.
- Acceptance: every path in 3.1 works; errors list valid names; `cargo test` is green.

### Phase 2: prose and polish

- Pager and `--no-pager`; prose for the most common leafs; a polished overview.
- Computed `referenced by`, `used in`, `try` (6.4).
- "Did you mean" suggestions.
- Acceptance: every implemented leaf has prose or appears in the to-do list; piping works with no pager.

### Phase 3: filtering and diagnostic hints

- `--chip`.
- `EspfError.help` and hints in the parser, context and options errors.
- `examples` command prints `espforge espf <tag> examples` pointers.
- Planned leafs (`led`, `button`, `ssd1306`, `wifi`, `http`) added with `status planned` and drift test 7.

### Phase 4: extras (optional)

- `--roff` and generated man pages for packaging.
- A `find` entry point (search over names and summaries) if it is wanted after all.
- Shell completion for sections and leafs.

## 10. Risks

- **Catalog drift.** The main risk. Mitigation: the drift tests in section 8, especially 7, 8, 9 and 11, which run in CI.
- **Writing burden.** Prose for dozens of leafs is real work. Mitigation: prose is optional, to-do test, phased release.
- **Promising unbuilt features.** Mitigation: the `planned` marker, or leave planned leafs out (11.2).
- **Reserved words.** `help`, `examples` and `overview` cannot be section or leaf names. Mitigation: drift test 5.
- **clap interaction.** `expand_help` must not swallow the trailing `help` for `espf`. Mitigation: test 14.

## 11. Open questions and ambiguities (from my side)

Settled since draft 2 and removed from this list: what `examples` does on a section (error, since a section's subcommands are its leafs), and where the format overview lives (`overview`).

### 11.1 Where does the catalog loader live?

`espforge-options` is the proposal. An alternative is a new `espforge-catalog` crate. The loader has no dependency that `espforge-options` lacks, so the extra crate is only a boundary. Do you want it?

### 11.2 What counts as "planned" versus "implemented", and do we list planned leafs at all?

Several example tags (`led`, `button`, `ssd1306`, `wifi`, `http`) are used by embedded examples but nothing implements them, and `build` refuses non-empty sections. Listing only what builds is honest but makes the browser look empty. Listing planned leafs shows direction but could promise things that change. My lean is to list them with a visible marker.

### 11.3 Hand-parsed trailing words, or a clap tree built from the catalog?

I proposed stripping `help` and `examples` ourselves and resolving a plain `Vec<String>`. The alternative builds a clap `Command` per section and leaf at startup, which gives free completion and typo suggestions and makes everything real subcommands, but couples clap to the catalog. Since the design is now "every level lists its subcommands", the clap tree fits more literally, at the cost of rewiring `help`. Which do you prefer?

### 11.4 Does the bare leaf print a title line?

You said a leaf should list the subcommands only. I print exactly the two commands (4.3). A section now has a description paragraph above its list, so there is a case for a one-line title above a leaf's two commands for consistency. Strictly the two lines, or a title too?

### 11.5 Is the section description always shown?

The bare section prints its description paragraph, then the leafs. When piped (for scripts), should the description be omitted so the output is only `command<TAB>description` lines? I lean to omitting it when stdout is not a terminal.

### 11.6 `--help` and `-h` after a path

Does `espforge espf peripherals i2c --help` mean the leaf's man page, or clap's generic `espf` usage? Mapping it to the man page is friendlier but hides the grammar; I lean to mapping `--help` after a path to `help`, and keeping clap's usage for `espforge espf --help` alone.

### 11.7 Two different things called `examples`

`espforge examples` copies whole example projects; `espforge espf <leaf> examples` prints snippets. That is consistent in spirit, but a newcomer may mix them up. The `try` line bridges them. Is the shared name fine? (You chose it, so I assume yes.)

### 11.8 Chip awareness of `pin` attributes

Pins are fixed per chip variant. Should `espforge espf --chip esp32c3 peripherals gpio help` show the valid pin range? That needs per-chip data the catalog does not have yet (possibly derivable from `esp-metadata-generated`). Left out of phases 1-3 except for `--chip` filtering whole leafs.

### 11.9 Repeated groups and dotted keys

`wifi` uses `network1.ssid`, `network2.ssid`. Document the numbered pattern as one attribute (`network<N>.ssid`), or list a couple of concrete ones? I propose a pattern syntax, which is new syntax in the catalog.

### 11.10 Duplicate tags across sections, and the shorthand

Today tags are unique across sections (`i2c_component` is a component, `i2c` a peripheral), which keeps the `espf i2c` shorthand unambiguous. Must they stay unique, or must the design allow the same tag in two sections? Also, do you want the shorthand at all, given the grammar is section then leaf?

### 11.11 Inconsistencies the catalog will expose

Reading the current code and examples, these already disagree, and someone must decide the truth before the catalog can describe them:

- `led` takes `pin="$gpio18"` in `blink` but `gpio="$gpio7"` in `button`.
- `spi_component`: the wrapper requires `mode`, but the examples omit it and put `mode` on `<spi>` instead; `cs` is `cs="9"` in the SPI examples, `cs="gpio5"` in `examples/example.espf.txt`, and `$gpio5` is tolerated.
- `i2c_component`: the wrapper requires `address`, but the I2C examples omit it.
- Some examples give an `<i2c>` peripheral and an `<i2c_component>` the same `id="i2c0"`. The interner maps one name to one handle, so the two silently share it. This may be a real bug, and it affects how ids are documented (and what the overview says about ids).
- The `wifi` element still appears under `# peripherals` in the HTTP example, while the format now puts it in `# services`.
- `ssd1306` uses `component=`, while other devices use `i2c_component=` / `spi_component=`.

For each I need to know whether the catalog should document the intended shape (and the code and examples get fixed) or the current behaviour.

### 11.12 Cheat sheet labels

cheat.sh normally puts a comment line above each snippet. ESPF has no standalone comment line, so I put the label as a trailing `// label` on the snippet's first element line, which keeps everything pasteable (4.4). That is less pretty than a heading line above. Prefer pasteable, or prettier output with non-ESPF heading lines?

### 11.13 What lives in `examples` versus `help`

I put reference material (attribute table, types, defaults, referenced by) only in leaf `help`, and only snippets plus `used in` and `try` in `examples`. If you want a short attribute table also at the top of `examples`, say so.

### 11.14 The overview: length and sections

The overview is one page of prose in 4.6. Is one page right, or do you want it split (for example a separate short page on references and ids)? I lean to one page for now, since a second word at the root would be a second reserved name.

### 11.15 Pager, colour, naming

- Pager: `overview` and leaf `help` page when stdout is a terminal and `$PAGER` is usable; everything else never pages.
- Colour: plain text only, or bold attribute names and a coloured `required` marker? If any, the terminal check is on stdout (the existing `diag::renderer()` checks stderr).
- Name: `espf` next to `espforge` may confuse; `ref` is the proposed alias. Is there a better primary name? Changing it later breaks any docs written now.

## 12. Suggested order of discussion

1. 11.11 (inconsistencies): it decides what the catalog says, and what the overview may claim about ids.
2. 11.2 and 11.10: what is listed and whether tags stay unique.
3. 11.3 and 11.15 (name): the shape and name of the command.
4. 11.4, 11.5, 11.6, 11.12, 11.13, 11.14: the small output decisions.
5. Everything else.
