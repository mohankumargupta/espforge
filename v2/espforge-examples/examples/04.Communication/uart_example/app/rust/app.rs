#[allow(unused_variables)]

use crate::{component, Context};

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    let uart = component!(ctx, uart0);

    logger.info("UART Example");
    uart.write("Hello\n");
}

pub fn forever(ctx: &mut Context) {
    let logger = ctx.logger;
    let delay = ctx.delay;
    let uart = component!(ctx, uart0);

    // buffer_until_newline() returns the number of bytes buffered so far; a
    // non-zero count means a line arrived.
    if uart.buffer_until_newline() > 0 {
        logger.info("Message received:");
        logger.info(uart.get_buffered_string());
        uart.clear_buffer();
    }

    delay.delay_ms(10);
}
