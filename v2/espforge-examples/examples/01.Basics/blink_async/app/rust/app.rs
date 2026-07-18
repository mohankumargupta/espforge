// User-owned application code. This file is merged into the generated project
// (it is NOT regenerated). The rest of `src/main.rs`/`src/lib.rs` is generated.
//
// Two LEDs blink at independent rates. Each LED is driven by its own embassy
// task spawned in `setup`, so the two intervals (500ms / 1000ms) run truly
// concurrently. `Led::toggle` takes `&self` (interior mutability in
// `espforge_runtime`), so a task can hold a shared `&'static Led`.

use crate::{Context, Delay, Led};
use embassy_executor::Spawner;

/// Drive one LED with a fixed blink period. Spawned as its own embassy task so
/// each LED's timing is independent of the others. `delay` is taken by value
/// because `Delay` is `Copy` — that avoids borrowing `ctx` for the task's
/// lifetime.
#[embassy_executor::task(pool_size = 2)]
async fn blink_led(led: &'static Led, delay: Delay, period_ms: u32) {
    loop {
        led.toggle();
        // `delay` is the async `Delay` (the project uses `runtime: embassy`), so
        // this must be `.await`ed — it yields instead of blocking the executor.
        delay.delay_ms(period_ms).await;
    }
}

/// Runs once at startup, before the generated `forever` loop. Spawn the two
/// independent blink tasks here.
pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    let delay = ctx.delay;
    let red_led = component!(ctx, red_led);
    let blue_led = component!(ctx, blue_led);
    spawner.spawn(blink_led(red_led, delay, 500).unwrap());
    spawner.spawn(blink_led(blue_led, delay, 1000).unwrap());
}

/// Reserved for per-tick work. The blink tasks above own the LEDs, so this loop
/// has nothing to do.
pub async fn forever(ctx: &'static Context) {
	ctx.delay.delay_ms(1000).await;
}
