#[warn(unused_variables)]
use crate::Context;

pub fn setup(ctx: &mut Context) {
}

pub fn forever(ctx: &mut Context) {
    if let Some(led) = config.component("red_led") {
        led.toggle();
    }


}
