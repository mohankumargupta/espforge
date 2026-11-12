#![allow(unused_variables)]

use crate::Context;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_9X18_BOLD},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    logger.info("SPI multi-device example");

    // Two parts on one controller. The panel applies mode 0 at 40MHz, the
    // accelerometer mode 3 at 5MHz; each does so before asserting its own CS.
    let tft = device!(ctx, tft);
    Rectangle::new(Point::zero(), Size::new(320, 240))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::new(0, 0, 20)))
        .draw(tft)
        .ok();

    match device!(ctx, accel).read() {
        Ok(reading) => {
            let (x, y, z) = reading.milli_g();
            logger.info(format_args!("ADXL345 {x} {y} {z} mg"));
            let style = MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::YELLOW);
            Text::new("ADXL345 online", Point::new(20, 40), style)
                .draw(tft)
                .ok();
        }
        Err(error) => {
            logger.warn(format_args!("ADXL345 read failed: {error:?}"));
        }
    }
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;

    match device!(ctx, accel).read() {
        Ok(reading) => {
            let (x, y, z) = reading.milli_g();
            let style = MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::WHITE);
            let tft = device!(ctx, tft);
            // `Text` takes a `&str`; the live values go to the log.
            Text::new("ADXL345 live", Point::new(20, 100), style)
                .draw(tft)
                .ok();
            ctx.logger.info(format_args!("ADXL345 {x} {y} {z} mg"));
        }
        Err(error) => ctx.logger.warn(format_args!("ADXL345 read failed: {error:?}")),
    }

    delay.delay_ms(200);
}
