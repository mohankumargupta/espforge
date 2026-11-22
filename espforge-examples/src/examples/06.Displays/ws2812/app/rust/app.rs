#![allow(unused_variables)]

use crate::Context;

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    logger.info("WS2812 RGB strip example");

    let strip = device!(ctx, strip);
    let pixels = [[32, 0, 0], [0, 32, 0], [0, 0, 32]];
    if strip.write(&pixels).is_err() {
        logger.error("WS2812 frame failed");
    }
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;
    delay.delay_ms(500);
}
