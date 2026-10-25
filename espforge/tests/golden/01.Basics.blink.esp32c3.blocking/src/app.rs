use crate::Context;

pub fn setup(ctx: &mut Context) {
    ctx.logger.info("blink example");
}

pub fn forever(ctx: &mut Context) {
    let led = component!(ctx, red_led);
    led.toggle();
    ctx.delay.delay_ms(1000);
}
