#[allow(unused_variables)]

use crate::{component, Context};

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    let uart = component!(ctx, uart0);

    uart.write_str("Hello\n").expect("uart write failed");
    logger.info("UART ready; type a line terminated by newline");
}

pub fn forever(ctx: &mut Context) {
    let logger = ctx.logger;
    let delay = ctx.delay;
    let uart = component!(ctx, uart0);

    // Line reading is backed by esp-hal's `read_buffered` + `\n` scan (§20.5).
    // `read_line` fills `buf` until a newline (or it is full) and returns the
    // number of bytes read.
    let mut buf = [0u8; 64];
    match uart.read_line(&mut buf) {
        Ok(n) if n > 0 => {
            logger.info("Received:");
            uart.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("")).ok();
        }
        Ok(_) => {}
        Err(_) => logger.info("UART read error"),
    }

    delay.delay_ms(10);
}
