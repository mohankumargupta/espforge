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
    logger.info("ILI9341 TFT example");

    // Landscape orientation: 320x240.
    let tft = device!(ctx, tft);
    Rectangle::new(Point::zero(), Size::new(320, 240))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::new(0, 0, 20)))
        .draw(tft)
        .ok();

    let title_style = MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::YELLOW);
    Text::new("ESPFORGE ILI9341", Point::new(20, 40), title_style)
        .draw(tft)
        .ok();
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;
    delay.delay_ms(1000);
}
