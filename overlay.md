That is a much cleaner mental model, and it matches what `espforge` **already does** for `src/app.rs`, `diagram.json`, `wokwi.toml`, `chip.json`, and `chip.wasm`.

If **everything the user controls lives next to the `.espf` file as an overlay**, then `build/` becomes 100% disposable, reproducible build output—like `target/`.

---

## 1. What You Can Delete Right Away

Once user edits live in project-root overlay files instead of inside `build/`:

- **Delete `Ownership` (`Owned` / `Merged` / `SeedOnce` / `NotOwned`)** from `espforge-emit/src/artifact.rs` and `plan.rs`.
- **Delete `espforge-emit/src/manifest.rs`, `.espforge/manifest.json`, and `ManifestDrift`** from `espforge-emit/src/lib.rs` and `espforge-core/src/project.rs`.
- **Make `write()` / `write_assets()` unconditionally write** generated + overlaid files into `build/`, just like `src/app.rs` (`espforge-core/src/project.rs:146`) and `chip.wasm` already do.

Users can safely `rm -rf build` or `.gitignore` `build/` at any time, and `.cargo/config.toml` is never stuck on stale chip, runner, or OTA settings.

---

## 2. How the Overlay Directory Next to `.espf` Looks

Expand `OVERLAY_FILES` in `espforge-core/src/source.rs` and `layer_overlay` in `espforge-emit/src/project.rs` from Wokwi-only to **all buildable project files**:

```text
my-project/
├── my_project.espf        # Hardware & service spec
├── src/app.rs             # Application logic (already copied verbatim)
├── drivers/               # Custom driver .kdl manifests (already supported)
│
│   # Optional user overlays (generated automatically when absent):
├── diagram.json           # Wokwi diagram (supports {{ board }}, {{ <id>.pin }}, etc.)
├── wokwi.toml             # Wokwi config
├── partitions.csv         # Custom ESP-IDF partition table (great for OTA + SPIFFS/NVS!)
├── rust-toolchain.toml    # Override Rust channel/targets
├── .cargo/
│   ├── config.toml        # Full override of cargo target/runner/rustflags
│   └── esp-config.toml    # Custom [env] vars (ESP_LOG, Wi-Fi creds, etc.)
└── Cargo.extra.toml       # Extra [dependencies], [patch], [profile], [lints] merged into build/Cargo.toml
```

*(Note: checking both `.cargo/esp-config.toml` and `esp-config.toml` at the project root is ergonomic if you don't want users having to create a hidden `.cargo/` directory next to `.espf`.)*

---

## 3. Two Subtle Details to Get Right

### A. `Cargo.toml` Overlay: Merge `Cargo.extra.toml` (or Root Keys) Instead of Raw Replace
If a user supplies a full `Cargo.toml` overlay that replaces `build/Cargo.toml` verbatim, adding a `<ssd1306>` or `<ota>` in `.espf` later won't add `espforge-drivers` or `esp-storage` to `build/Cargo.toml`.

Instead, reuse the `toml_edit` merger you already wrote in `espforge-emit/src/codegen/cargo.rs::merge`, just **flipped to read from the project root**:
1. Generate the authoritative `Cargo.toml` from `Board` + `Meta`.
2. If `Cargo.extra.toml` (or a user overlay TOML) exists next to `.espf`, merge the user's extra `[dependencies]`, `[patch]`, `[profile]`, and `[lints]` into it before writing `build/Cargo.toml`.
3. `build/Cargo.toml` itself is always overwritten cleanly—no reading back from `build/`.

### B. `partitions.csv` + OTA Runner Interaction
Putting `partitions.csv` in the overlay system solves a big OTA ergonomics problem:
- When `<ota />` is enabled, `espforge` generates a sensible default `partitions.csv` (`nvs`, `otadata`, `phy_init`, `ota_0`, `ota_1`).
- If the user wants a custom partition layout (e.g., adding a `littlefs` / `spiffs` data partition or asymmetric OTA slots), they drop their own `partitions.csv` next to `.espf` and `layer_overlay` copies it over the generated one.
- In `espforge-emit/src/codegen/config.rs`, enable `--partition-table partitions.csv` in the `espflash` runner whenever **either** `opts.is_on("ota")` **or** the overlay contains `partitions.csv` (that way a user using a custom `partitions.csv` even without `<ota />` gets flashed with their table too).

---

## Summary

Deleting `Ownership` + `manifest.json` + `ManifestDrift` and moving customization into **project-root overlay files** (with `Cargo.extra.toml` merged by `cargo::merge`) gives users total control in Git-tracked source files, eliminates `build/` drift errors, and makes OTA (`partitions.csv` + `.cargo/config.toml` updates) completely seamless.
