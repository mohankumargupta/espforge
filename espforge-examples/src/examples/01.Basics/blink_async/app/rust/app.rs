use crate::{component, Context};
use embassy_executor::Spawner;

pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    ctx.logger.info("blink async example");
}

pub async fn forever(ctx: &'static Context) {
    let led = component!(ctx, red_led);
    led.toggle();
    ctx.delay.delay_ms(1000).await;
}
