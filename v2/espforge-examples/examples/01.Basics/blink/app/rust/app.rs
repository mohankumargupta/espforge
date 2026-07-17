use crate::{Context};

pub fn setup(_ctx: &mut Context) {
    // component and device macros available globally
}

pub fn forever(ctx: &mut Context) {
    let led = component!(ctx, red_led);
    loop {
        led.toggle();
        ctx.delay.delay_ms(1000);
    }
}
