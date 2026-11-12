#![allow(unused_variables)]

use crate::Context;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::BinaryColor,
    prelude::*,
    text::Text,
};

/// Draw one line of status text on the panel.
fn status(ctx: &mut Context, line: &str) {
    let oled = device!(ctx, oled);
    let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    oled.clear();
    Text::new(line, Point::new(0, 10), style)
        .draw(&mut *oled)
        .ok();
    oled.flush().ok();
}

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    logger.info("I2C multi-device example");

    // The sensor is deferred: its constructor (and any bus probe) runs here,
    // after boot, as a `Result` this app handles.
    match device!(ctx, air).init() {
        Ok(air) => match air.start() {
            Ok(()) => {
                logger.info("SCD4x measuring");
                status(ctx, "SCD4x online");
            }
            Err(error) => {
                logger.warn(format_args!("SCD4x start failed: {error:?}"));
                status(ctx, "SCD4x error");
            }
        },
        Err(error) => {
            logger.warn(format_args!("SCD4x init failed: {error}"));
            status(ctx, "SCD4x absent");
        }
    }
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;

    // A second `init()` returns the same slot, so the loop reads the device
    // rather than rebuilding it. The panel's borrow is separate and starts
    // after this one ends.
    match device!(ctx, air).init() {
        Ok(air) => match air.read() {
            Ok(measurement) => ctx.logger.info(format_args!(
                "CO2 {} ppm, {} C, {} %RH",
                measurement.co2_ppm,
                measurement.temperature_centi_c as f32 / 100.0,
                measurement.humidity_centi_percent as f32 / 100.0
            )),
            Err(error) => ctx.logger.warn(format_args!("SCD4x read failed: {error:?}")),
        },
        Err(error) => ctx.logger.warn(format_args!("SCD4x init failed: {error}")),
    }

    delay.delay_ms(5000);
}
