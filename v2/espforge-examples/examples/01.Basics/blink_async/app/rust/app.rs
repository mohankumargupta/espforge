// Two LEDs blink at independent rates. 
// Two intervals (500ms / 1000ms)

use crate::{Context, Delay, Led};
use embassy_executor::Spawner;

/// Drive one LED with a fixed blink period.
/// pool_size is 2 because 2 leds
/// `delay` is taken by value because `Delay` is `Copy` — that avoids borrowing `ctx` for the task's lifetime.
#[embassy_executor::task(pool_size = 2)]
async fn blink_led(led: &'static Led, delay: Delay, period_ms: u32) {
    loop {
        led.toggle();
        delay.delay_ms(period_ms).await;
    }
}

pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    let delay = ctx.delay;
    let red_led = component!(ctx, red_led);
    let blue_led = component!(ctx, blue_led);
    spawner.spawn(blink_led(red_led, delay, 500).unwrap());
    spawner.spawn(blink_led(blue_led, delay, 1000).unwrap());
}

pub async fn forever(ctx: &'static Context) {
	ctx.delay.delay_ms(1000).await;
}
