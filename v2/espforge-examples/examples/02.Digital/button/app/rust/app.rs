#[warn(unused_variables)]

use crate::{component, Context};

pub fn setup(ctx: &mut Context) {
  let logger = ctx.logger;
  logger.info("Button Example");
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;
    let button = component!(ctx, button);
    let led = component!(ctx, red_led);
    
    if button.is_pressed() {
        led.toggle();
        delay.delay_ms(100);
    }
    
    delay.delay_ms(100);
}
