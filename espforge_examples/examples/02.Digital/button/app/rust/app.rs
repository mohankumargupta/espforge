#[warn(unused_variables)]

use crate::{component, Context};

pub fn setup(ctx: &mut Context) {
  ctx.logger.info("Button Example");
}

pub fn forever(ctx: &mut Context) {
    let logger = ctx.logger;
    let delay = ctx.delay;
    let button = component!(button);
    let led = component!(red_led);
    
    if button.is_button_pressed() {
        led.toggle();
        delay.delay_ms(100);
    }
    
    delay.delay_ms(100);
}
