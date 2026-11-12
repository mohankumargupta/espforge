# Journey: a driver package

This walkthrough follows one driver that does not live in the bundled schema:
`testing/fixtures/drivers/accel.kdl` beside its crate
`testing/fixtures/fx-accel-driver`, used by
`testing/fixtures/local-driver/project.espf`.

The point of the phase is that the tag, the crate and the manifest's bytes are
all recorded. A project can use a bundled driver, a project driver or a fetched
one, and be able to say which one it used.

> **Phase 8 update:** the manifest below is the v1 shape this walkthrough
> introduced. It still loads (with a deprecation warning), but new drivers use
> the `driver` format in
> [`journey-driver-manifest-v2.md`](journey-driver-manifest-v2.md). The v2
> fixture manifest is `testing/fixtures/drivers/accel.kdl`; `espforge driver
> migrate` produces it from the v1 file.

---

## 1. The spec

```espf
# espforge
  <project name="local_driver" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals
  <i2c id="i2c0" sda="6" scl="7" frequency="400kHz" />

# devices
  <fx_accel id="accel" bus="$i2c0" address="0x10" />

# services
```

`fx_accel` is not in `espf.schema.kdl`, `services.schema.kdl` or
`espforge-drivers/drivers.schema.kdl`. Without a manifest, lowering says the
tag is not an element.

---

## 2. The manifest

```kdl
element "fx_accel" section="devices" {
    summary "Fixture accelerometer from a project driver package"
    crate {
        package "espforge-driver-fx-accel"
        path "../fx-accel-driver"
    }
    expose "embedded-graphics" { version "0.8" }
    device_recipe {
        bus "i2c"
        feature "fx-accel"
        runtime "blocking"
        kind "sensor"
        type "espforge_driver_fx_accel::FxAccel"
        new "espforge_driver_fx_accel::FxAccel::new"
        needs_address
    }
    attr "id" kind="ident" required=#true
    attr "bus" kind="ref" to="i2c" required=#true
    attr "address" kind="hex" required=#true
    attr "init" kind="choice" default="boot" { choices "boot" "deferred" }
    example "minimal" #"<fx_accel id="accel" bus="$i2c0" address="0x10" />"#
}
```

Three things are load-bearing:

- `crate` names the Rust package, and exactly one of `version`, `path` or
  `git` + `rev`. The schema refuses a manifest that names none or several, so
  the generated dependency line is never ambiguous.
- `expose` names a crate the driver's public types come from; the generated
  project names it once, even with two devices from the same package.
- `device_recipe` is unchanged from a bundled driver. The lowerer that reads it
  knows no tag, so an external driver is the same data in a different file.

`needs_address` makes the device declare its own 7-bit address, which lowering
checks against the usable range and passes as the constructor's second
argument.

---

## 3. Where the tag comes from

`espforge-core/src/drivers.rs` collects four tiers in precedence order:

| Precedence | Source | Where |
|---|---|---|
| 1 | project | `<spec dir>/drivers/*.kdl` |
| 2 | explicit | `--driver-dir <dir>` (repeatable) |
| 3 | cache | `$XDG_CACHE_HOME/espforge/drivers/<tag>/<version>/driver.kdl` |
| 4 | bundled | `espforge-drivers/drivers.schema.kdl` |

The labels are paths, so the composed schema records the file each element came
from (`Element::source`). A tag declared twice is one `SchemaError` naming both
files; precedence is not a shadowing rule, because silently changing which
driver a tag means is exactly the failure this phase exists to prevent.

`cargo run -p espforge -- check testing/fixtures/local-driver/project.espf \
    --driver-dir testing/fixtures/drivers` composes the bundled files with the
fixture manifest. The element reaches the registry through the same
`device_recipes()` door a bundled one uses; `generated::validate` returns `None`
for it, and `espforge-elements/src/dynamic_attrs.rs` validates it from the
schema instead. The wording is the same `Issue` the generated readers produce,
so a missing or invalid attribute reads the same whether the driver is bundled
or not.

---

## 4. The generated Cargo.toml

The package's `path` is relative to the manifest, resolved and containment
checked by `espforge-core::drivers::resolve_package_paths`, then written as an
absolute path:

```toml
espforge-driver-fx-accel = { path = "/…/testing/fixtures/fx-accel-driver" }
embedded-graphics = "0.8"
```

There is no `espforge-drivers` line: the fixture is not a bundled driver, so
its feature does not belong to the umbrella crate. A project that uses both
gets both lines, and two devices from the fixture share the one package line.

A `path` crate is the development form. The shareable form is `version` (a
crates.io release) or `git` + `rev`; both emit the dependency line cargo
expects.

---

## 5. The lockfile

```json
{
  "version": 1,
  "drivers": {
    "fx_accel": {
      "source": "testing/fixtures/drivers/accel.kdl",
      "crate": {
        "package": "espforge-driver-fx-accel",
        "path": "../fx-accel-driver"
      },
      "manifest_sha256": "4e0708…"
    }
  }
}
```

`gen` writes it to `.espforge/drivers.lock` in the generated project. A later
`gen` recomputes the hash of every manifest it reads and compares:

- a changed manifest is `DRIVER_LOCK_STALE`, with the expected and found
  hashes and `espforge driver update <tag>` as the fix;
- a manifest the lock does not mention is `DRIVER_LOCK_MISSING`, with
  `espforge driver add --local <manifest>` as the fix;
- no lock at all is the first generation, which records one.

The lock is about espforge's inputs, not Rust resolution: the generated
project's own `Cargo.lock` stays cargo's business.

---

## 6. Adding a driver

```sh
espforge driver add testing/fixtures/drivers/accel.kdl   # copy into drivers/
espforge driver add --local path/to/manifest.kdl         # record in place
espforge driver list                                     # tag, crate, source
espforge driver update fx_accel                          # after editing it
espforge driver remove fx_accel                          # forget and delete the copy
```

`add` validates the manifest against the core schema before writing anything,
so a duplicate tag or a malformed recipe is refused with the schema's own span.
`gen`, `build` and `check` never fetch: they read `drivers/`, `--driver-dir`,
the cache and the lock.

---

## 7. The trust boundary

A manifest is executable input: its Rust type and constructor paths and its
dependency block are injected into generated code. Phase 7 therefore:

- fetches only through an explicit `driver add` (the first release uses the
  directory form; archive download is deferred);
- accepts `https://` and `file://` sources in the lock, and a local path only
  when it is inside the project or the explicit driver directory;
- records a checksum and refuses a changed manifest;
- does not sign. A signature without a published key and a revocation story is
  theater; the checksum is the half that protects a build today. Phase 8 owns
  identity, a registry index and semver ranges.

The rule the phase leaves behind: **any byte that changes generated code is
either in the lockfile or in the curated schema, and a driver the project did
not explicitly add never runs.**

---

## 8. Build

```sh
espforge gen testing/fixtures/local-driver/project.espf \
    --driver-dir testing/fixtures/drivers \
    --runtime-path "$PWD/espforge-runtime" --out target/p7-local
cd target/p7-local && cargo build --target riscv32imc-unknown-none-elf
```

The generated `src/generated.rs` names `espforge_driver_fx_accel::FxAccel` and
its constructor; the address is a constant argument the lowerer inserted. No
`espforge-drivers` feature is involved, and the boot log names `accel` if the
constructor returns an error.
