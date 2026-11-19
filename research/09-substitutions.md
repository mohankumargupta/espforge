# Substitutions: `env:NAME` and `substitutions.env`

Status: **implemented** (`espforge-core::substitutions`, the `E12xx` codes, the
`substitutions.env` template `espforge new` writes, and the README section).
This document records the behaviour the parser, the CLI and the docs share.

## The problem

A Wi-Fi project currently hard-codes its credentials in the spec:

```xml
<wifi_station id="home" wifi="$radio" ssid="Wokwi-GUEST" auth="open" />
```

Two things are wrong with putting a real network there:

* the spec is normally committed, so the SSID (and password) go into git;
* the same spec cannot be built on a laptop and on a test bench without editing
  it, which is exactly the edit that gets committed by accident.

ESPHome solves this with a `substitutions:` block plus `!secret`. We do not have
a YAML mapping, so the smallest equivalent is a marker in the attribute value and
one file beside the spec.

## The proposal

Any attribute value in a `.espf` / `.espf.txt` file may be written as
`env:NAME`. `espforge` replaces it with the value of `NAME` before the spec is
lowered:

```xml
<wifi_station id="home" wifi="$radio"
              ssid="env:WIFI_SSID"
              password="env:WIFI_PASSWORD" />
```

with `substitutions.env` next to the spec:

```dotenv
# substitutions.env -- do not commit this file
WIFI_SSID=MyNetwork
WIFI_PASSWORD="correct horse battery staple"
```

The spec then contains no secret and no site-specific string, and the same file
builds anywhere. `substitutions.env` is read from the directory of the `.espf`,
the same directory the overlay files come from (`src/app.rs`,
`.cargo/config.toml`, `partitions.csv`, ...).

## Current state (what has to change)

| Today | After |
|---|---|
| Only `<wifi_station password="env:NAME">` understands `env:`. It does not read any file: it emits `env!("NAME")` into `src/generated.rs`, so cargo resolves it at build time. | `env:NAME` is resolved by `espforge` while reading the spec, for every attribute. The generated Rust receives a plain literal. |
| `ssid="env:WIFI_SSID"` is accepted as a *literal SSID* of 13 bytes named `env:WIFI_SSID`. It builds and fails to connect. | The same text resolves to the value, or is reported as a missing key. |
| A missing variable surfaces as a cargo `env!` error in `build/`, pointing at generated code. | `espforge check` reports it, pointing at the attribute in the spec. |

The password case is the only behavioural regression to weigh: today the secret
never appears in generated source, only in the binary. After the change it is
also written verbatim into `build/src/generated.rs`. That directory is
disposable and already regenerated on every build, so the recommendation is to
take the uniform rule and, if keeping secrets out of generated source matters
later, add an explicit `secret:NAME` marker that emits `env!("NAME")`. Two
markers with two honest meanings beat one marker with two behaviours.

## File format

`substitutions.env` is a dotenv-shaped `KEY=VALUE` list, deliberately smaller
than dotenv:

```dotenv
# comments run to end of line
# blank lines are ignored

WIFI_SSID=MyNetwork
WIFI_PASSWORD="correct horse battery staple"
LED_PIN=18
```

Rules:

* one `KEY=VALUE` per line; the **first** `=` splits (so values may contain `=`).
* `KEY` must be `[A-Za-z_][A-Za-z0-9_]*`. Anything else is an error naming the
  line.
* `#` at the start of a line (after optional spaces) is a comment. There are no
  trailing comments: `A=b#c` means the value `b#c`, because Wi-Fi passwords
  contain `#`.
* surrounding double quotes are stripped when both ends have one, which is how a
  value keeps leading/trailing spaces or starts with `#`. There are no escape
  sequences: a value is used exactly as written between the quotes.
* unquoted values are trimmed of leading and trailing spaces and tabs.
* no variable expansion (`$OTHER` is literal), no `export ` prefix, no `source`,
  no multiline values.
* a repeated key takes the last value.
* UTF-8, CRLF and a leading BOM tolerated.

If a line does not parse, the whole file is rejected with the file name and line
number. A half-read substitutions file is worse than a loud failure.

## Resolution rules

1. **Whole value only.** An attribute is a substitution when its value is
   exactly `env:NAME`. `espf-env:NAME`, `env: NAME`, `network-env:X` and an SSID
   that merely contains `env:` are literals.
2. **Any attribute, any section.** `<project>`, `<chip>`, `<runtime>`,
   `<options>`, peripherals, devices and services are treated alike, including
   `id`, `pin` and `name`. Resolved text is then checked exactly as if the user
   had typed it, so `pin="env:LED_PIN"` must resolve to a pin the board has, and
   `id="env:NAME"` must resolve to a Rust identifier. A value that is empty
   after resolution is checked by the attribute's own rule (an empty SSID is a
   `COMPONENT_INVALID_SSID`, not a substitution error).
3. **One pass, not recursive.** The substituted text is final; if
   `WIFI_SSID=env:OTHER`, the SSID is the literal string `env:OTHER`. This is
   the cheapest way to guarantee no cycles, and it keeps `env:` meaning one
   thing.
4. **References still work.** Because the resolved text is treated as typed,
   `wifi="env:RADIO"` with `RADIO=$radio` is a valid reference. That is a
   feature: the radio id can differ per board.
5. **Precedence: process environment first, then the file.** `WIFI_SSID=Lab
   espforge build` overrides the file without editing it, which is what CI
   needs. A variable that is set but empty counts as set.
6. **Not in prose.** Comment blocks and section headers are documentation and
   are never substituted.
7. **Lazy.** The file is read only when the spec actually contains `env:`. A
   project that does not use substitutions neither needs the file nor breaks
   because of a malformed one.
8. **Only for file-based specs.** `espforge gen --example blink` has no project
   directory, so there is nowhere to look for `substitutions.env`; for examples
   `env:` resolves from the process environment alone.

## Diagnostics

Substitution runs once, between parsing and lowering, so both `espforge check`
and `espforge build`/`gen` see the same errors. The span points at the
`env:NAME` value in the spec (the `.env` file has no source map; it is named in
the message instead).

| Situation | Reported as |
|---|---|
| `env:` appears in the spec, `substitutions.env` is absent, and the process environment does not define the key | error on the attribute: `env:WIFI_SSID` is not defined in `substitutions.env` or the environment |
| the same key twice in the file | the last value wins; no warning |
| a line in `substitutions.env` is not `KEY=VALUE` or has a bad key | error naming `substitutions.env:<line>` and the offending text |
| `env:` with no name, or a name that is not an identifier | error on the attribute, as `COMPONENT_INVALID_ENV` does today for passwords |

New diagnostic codes are needed for the spec-side and file-side errors; propose
a `E12xx` group (`SUBSTITUTION_*`) rather than stretching the component `E06xx`
range, whose numbering is about elements, and update the `codes!` slice and its
`the_slice_only_defines_the_codes_it_can_reach` range in the same change.

Note that a missing variable becomes an *espforge* error rather than a cargo
error. That is the point: `check` can fail before anything is generated.

## Secrets

A substituted value is compiled into the firmware. There is no way around that
for an SSID or a WPA2 password: the device must send it. The practical rules:

* `substitutions.env` must be git-ignored. `espforge new` should write a
  commented-out template and add the name to the project's `.gitignore`.
* Quote values that contain spaces; write a `#` inside a value directly
  (`A=b#c`), since only whole-line comments are recognised.
* `substitutions.env` is *not* an overlay file. It is never copied into
  `build/`, and editing it does not require touching the generated project.
* The existing schema help for `password` ("the value is compiled into the
  firmware") stays true and should be moved to the general substitution
  documentation.

## Explicitly out of scope

**A context variable in `src/app.rs`.** Tempting, and rejected for now. Exposing
substitutions as, say, `ctx.config.wifi_ssid` would (a) generate a stringly API
whose fields depend on which `env:` values a spec happens to contain, (b) put
secrets into a runtime struct that app code can log, and (c) freeze the generated
`Context` shape around a build-time concern. If firmware code eventually needs
one of these values, the safer design is an explicit declaration listing the
keys the app may see, lowered into a small generated struct, rather than
exporting every substitution.

**`.cargo/config.toml` `[env]`.** That file is an overlay: `.cargo/config.toml`
is copied verbatim, so values placed there are cargo's business, are not
substituted into the spec, and would be a second, differently-scoped source of
the same names. Two lookup paths for `WIFI_SSID` is exactly the confusion this
proposal avoids. If a *non-secret* value is needed by `build.rs` or by a
`include_str!`-style path, put it in `.cargo/config.toml` deliberately; keep
`substitutions.env` for spec values.

## Implementation sketch (for later)

1. `espforge-core`: a `substitutions` module with `load(dir) -> Option<Table>`,
   the dotenv parser above, and its own tests. `Source` carries the directory
   already, so no plumbing changes.
2. `espf` / a new pass in `espforge-core`: walk `Parsed::sections`, rewrite
   `ParsedNode.attributes[].val` when it is exactly `env:NAME`, and re-point
   `ParsedNode.id` if the `id` attribute was substituted. Keep the original
   offsets so diagnostics still land on the value; the substituted text is owned
   (`String`), so this pass needs either a borrowed-until-now AST or an owned
   copy — an owned `ParsedNode` for this stage is the simpler answer.
3. Remove the special case in `espforge-elements/src/services/wifi_station.rs`
   (`Password::Env` and its `env!` tokens), let `password` be a plain `String`,
   and update `COMPONENT_INVALID_ENV`'s wording or retire it.
4. Update goldens for the wifi examples (`UPDATE_GOLDENS=1`) and the schema help
   text for `ssid` / `password`.
5. Docs: link this file from `README.md` next to the overlay-file paragraph, and
   replace the "env:NAME to read NAME at build time" help with a pointer here.
