#[warn(unused_variables)]

use crate::Context;

pub fn setup(ctx: &mut Context) {
    ctx.logger.info("Starting Blink Example");
}

pub fn forever(ctx: &mut Context) {
    // Access the red_led defined in example.yaml
    ctx.components.red_led.toggle();
    
    // Use the delay from context
    ctx.delay.delay_ms(1000);
}

