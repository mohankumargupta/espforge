#[allow(unused_variables)]

use crate::{component, Context};
use embassy_executor::Spawner;

pub async fn setup(ctx: &mut Context, _spawner: Spawner) {
    ctx.logger.info("Starting Embassy Blink Example");
}

pub async fn forever(ctx: &mut Context) {
    let led = component!(red_led);
    led.toggle();
    ctx.delay.delay_ms(1000).await;
}

