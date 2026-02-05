#[allow(unused_variables)]

use crate::{component, Context};

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;

    logger.info("Starting Blink Example");
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;
    let led = component!(red_led);

    led.toggle();
    delay.delay_ms(1000);
}

