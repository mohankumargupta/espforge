#![allow(unused_variables)]

use crate::Context;

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    let i2c = component!(ctx, i2c_tester);

    logger.info("I2C Example");

    // Write one byte (register index 0x03) to address 0x42,
    // then read one byte back. The custom I2C chip echoes the
    // register value at the written index, similar to the SPI chip.
    let mut buffer = [0x00u8];
    let sent_byte = 0x03u8;
    match i2c.write_read(0x42, &[sent_byte], &mut buffer) {
        Ok(_) => {
            logger.info(format_args!("Sent {}", sent_byte));
            logger.info(format_args!("Received: {}", buffer[0]));
        }
        Err(_) => {
            logger.info("I2C transfer failed");
        }
    }
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;
    delay.delay_ms(1000);
}
