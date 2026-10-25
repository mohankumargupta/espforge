//! Embassy app logic for the compile-gate fixture.
//!
//! Every embedded example's `app.rs` is blocking (`setup(ctx: &mut Context)`,
//! `forever(ctx: &mut Context)`), so forcing `--runtime embassy` on one
//! produces a project that cannot compile. This fixture is the embassy
//! counterpart: the same LED spec, with the async app the runtime expects.

use crate::Context;
use embassy_executor::Spawner;

pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    ctx.logger.info("embassy fixture");
}

pub async fn forever(ctx: &'static Context) {
    let led = component!(ctx, red_led);
    loop {
        led.toggle();
        ctx.delay.delay_ms(500).await;
    }
}
