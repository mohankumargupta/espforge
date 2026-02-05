#[warn(unused_variables)]

use crate::{component, Context};
use embassy_executor::Spawner;

use espforge_platform::signal;

signal!(BUTTON_PRESSED);

pub async fn setup(ctx: &mut Context, spawner: Spawner) {
  spawner.spawn(button_task()).ok();
}

pub async fn forever(ctx: &mut Context) {
   let red_led = component!(red_led);
   BUTTON_PRESSED.wait().await;
   red_led.toggle();
   ctx.delay.delay_ms(100).await;
}

#[embassy_executor::task]
async fn button_task() {
  let button = component!(button);
  
  loop {
        button.wait_for_pressed().await;
        BUTTON_PRESSED.signal();
    }
}
