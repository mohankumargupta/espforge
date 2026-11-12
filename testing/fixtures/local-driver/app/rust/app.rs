#![allow(unused_variables)]

use crate::Context;

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    logger.info("local driver example");
    match device!(ctx, accel).read() {
        Ok(value) => logger.info(format_args!("fixture reading {value}")),
        Err(error) => logger.warn(format_args!("fixture read failed: {error:?}")),
    }
}

pub fn forever(_ctx: &mut Context) {}
