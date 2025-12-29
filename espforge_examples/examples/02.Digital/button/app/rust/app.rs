#[warn(unused_variables)]

use crate::Context;

pub fn setup(ctx: &mut Context) {
  ctx.logger.info("Button Example");
}

pub fn forever(ctx: &mut Context) {
    let button = &mut ctx.components.button;
    let red_led = &mut ctx.components.red_led;
    if button.is_button_pressed() {
        red_led.toggle();
    }
}
