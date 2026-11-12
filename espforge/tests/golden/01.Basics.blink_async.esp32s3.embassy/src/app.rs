use crate::{device, Context};
use embassy_executor::Spawner;

pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    ctx.logger.info("blink async example");
}

pub async fn forever(ctx: &'static Context) {
    // The guard may be held across the await: it is a mutex guard, and a
    // spawned task's future stays `Send`.
    let mut led = device!(ctx, red_led);
    led.toggle();
    ctx.delay.delay_ms(1000).await;
}
