#![allow(unused_variables)]

use crate::Context;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, PrimitiveStyle},
    text::Text,
};

pub async fn setup(ctx: &'static Context, _spawner: embassy_executor::Spawner) {
    let logger = ctx.logger;
    logger.info("async SSD1306 example");

    // The panel is always deferred: `Devices::new` is synchronous, so the
    // constructor runs here, after the executor starts. The guard is the
    // device's `AsyncMutex` guard, so it may be held across `.await`.
    let mut slot = device!(ctx, oled);
    let oled = match slot.init().await {
        Ok(oled) => oled,
        Err(error) => {
            logger.error(format_args!("SSD1306 init failed: {error}"));
            return;
        }
    };

    let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    Circle::new(Point::new(4, 4), 24)
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(oled)
        .ok();
    Text::new("ESPFORGE ASYNC", Point::new(36, 20), text_style)
        .draw(oled)
        .ok();

    // The transfer itself awaits the bus.
    if let Err(()) = oled.flush().await {
        logger.warn("SSD1306 flush failed");
    }
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(1000).await;
}
