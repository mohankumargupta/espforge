#![allow(unused_variables)]

use crate::Context;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, PrimitiveStyle},
    text::Text,
};

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    logger.info("SSD1306 OLED example");

    let oled = device!(ctx, oled);
    let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    // The panel keeps a framebuffer; draw calls only change it.
    Circle::new(Point::new(4, 4), 24)
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(oled)
        .ok();

    Text::new("ESPFORGE", Point::new(36, 20), text_style)
        .draw(oled)
        .ok();

    // Push the framebuffer to the panel.
    oled.flush().ok();
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;
    delay.delay_ms(1000);
}
