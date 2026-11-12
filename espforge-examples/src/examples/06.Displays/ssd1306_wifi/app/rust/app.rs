#![allow(unused_variables)]

use crate::Context;
use embassy_executor::Spawner;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::BinaryColor,
    prelude::*,
    text::Text,
};

/// Draw one line of status text.
///
/// The panel is deferred, so the first call also awaits its constructor. The
/// guard is the device's `AsyncMutex` guard, so holding it across
/// `flush().await` is safe.
async fn status(ctx: &'static Context, line: &str) {
    let mut slot = device!(ctx, oled);
    let oled = match slot.init().await {
        Ok(oled) => oled,
        Err(error) => {
            ctx.logger.warn(format_args!("SSD1306 init failed: {error}"));
            return;
        }
    };

    let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    oled.clear();
    Text::new(line, Point::new(0, 10), style)
        .draw(&mut *oled)
        .ok();
    oled.flush().await.ok();
}

pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    let logger = ctx.logger;
    logger.info("SSD1306 + Wi-Fi example");

    // The panel is built here, after the executor started, then drawn.
    status(ctx, "connecting").await;

    logger.info("Waiting for Wi-Fi connection...");
    service!(ctx, home).wait_connected().await;
    logger.info("Connected");

    status(ctx, "online").await;
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(1000).await;
}
