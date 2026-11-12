//! Compile gate for the unified device guard.
//!
//! `device!` is one macro for every runtime and every device: a blocking board
//! hands out `&mut`, an embassy board awaits the device's `AsyncMutex`. These
//! tests generate real projects, replace one app file, and assert the compiler
//! accepts the shapes the unified macro promises.
//!
//! They are `#[ignore]` for the same reason the golden compile gate is: they
//! shell out to `cargo` for a riscv target and need the dependency cache. Run
//! them in the release gate:
//!
//! ```sh
//! cargo test -p espforge --test compile_fail -- --ignored
//! ```
//!
//! No `trybuild`: these crates are `no_std` riscv and `trybuild` compiles for
//! the host, where `esp-hal` is invisible.

use std::path::{Path, PathBuf};
use std::process::Command;

use espforge_core::answers::Overrides;
use espforge_core::project::generate_with;
use espforge_core::source::resolve_source;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the espforge crate lives in the workspace root")
        .to_path_buf()
}

/// Generate `example` into a throwaway project and make it its own workspace.
fn generate(example: &str, name: &str) -> PathBuf {
    let out = workspace().join("target/compile-fail").join(name);
    if out.exists() {
        std::fs::remove_dir_all(&out).expect("the previous case is removable");
    }
    let source = resolve_source(None, Some(example), Path::new("."))
        .unwrap_or_else(|e| panic!("resolving {example}: {e}"));
    generate_with(
        &source,
        &out,
        &Overrides::default(),
        Some(workspace().join("espforge-runtime")),
    )
    .unwrap_or_else(|e| panic!("generating {example}: {e:?}"));
    // The generated crate is inside espforge's own workspace directory; give
    // it a workspace root of its own so cargo does not try to join ours.
    let manifest = std::fs::read_to_string(out.join("Cargo.toml")).expect("a manifest was written");
    std::fs::write(out.join("Cargo.toml"), format!("{manifest}\n[workspace]\n"))
        .expect("the manifest is writable");
    out
}

fn write_app(dir: &Path, app: &str) {
    std::fs::write(dir.join("src/app.rs"), app).expect("the app is writable");
}

/// Build the case, asserting the unified guard compiles.
fn build_success(dir: &Path) -> String {
    let output = Command::new(env!("CARGO"))
        .args(["build", "--quiet"])
        .current_dir(dir)
        .env(
            "CARGO_TARGET_DIR",
            workspace().join("target/compile-fail/target"),
        )
        .output()
        .expect("cargo runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "the unified device guard did not compile:\n{stderr}"
    );
    stderr
}

/// A guard held across `.await` inside a spawned task: the future must stay
/// `Send`, which the old `RefCell` borrow could not. `spawner.spawn` is the
/// assertion.
#[test]
#[ignore = "compiles a mutated riscv project; run with `-- --ignored`"]
fn a_device_guard_may_be_held_across_await() {
    let dir = generate("01.Basics/blink_async", "guard-across-await");
    write_app(
        &dir,
        r#"use crate::{device, Context};
use embassy_executor::Spawner;

/// Holds the LED guard across a timer await: the task future must be `Send`.
#[embassy_executor::task]
async fn blink_task(ctx: &'static Context) {
    let mut led = device!(ctx, red_led);
    loop {
        led.toggle();
        ctx.delay.delay_ms(1000).await;
    }
}

pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    spawner.spawn(blink_task(ctx).unwrap());
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(1000).await;
}
"#,
    );
    build_success(&dir);
}

/// `device!` on an async device awaits the lock, so the same spelling the
/// blocking examples use also works on an async panel.
#[test]
#[ignore = "compiles a mutated riscv project; run with `-- --ignored`"]
fn device_works_on_an_async_panel() {
    let dir = generate("06.Displays/ssd1306_async", "device-on-async");
    write_app(
        &dir,
        r#"use crate::Context;

pub async fn setup(ctx: &'static Context, _spawner: embassy_executor::Spawner) {
    let mut slot = device!(ctx, oled);
    let _panel = match slot.init().await {
        Ok(panel) => panel,
        Err(_) => return,
    };
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(1000).await;
}
"#,
    );
    build_success(&dir);
}
