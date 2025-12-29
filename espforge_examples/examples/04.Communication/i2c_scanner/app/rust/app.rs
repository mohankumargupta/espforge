#![no_std]
#![no_main]

#[warn(unused_variables)]

use crate::Context;
use embedded_hal::i2c::I2c;

pub fn setup(ctx: &mut Context) {
    ctx.logger.info("I2C Scanner Example");
    
    let i2c = &mut ctx.components.my_i2c;
    let logger = &ctx.logger;

    logger.info("Scanning I2C bus...");

    // Scan standard 7-bit addresses (1 to 127)
    for address in 1..128u8 {
        // Probe by writing 0 bytes. If the device is present, it will ACK the address.
        match i2c.write(address, &[]) {
            Ok(_) => {
                logger.info(format_args!("Found device at address 0x{:02x}", address));
            }
            Err(_) => {
                // Ignore errors (NACK means no device responded)
            }
        }
        // Small delay to not flood the log too fast
        ctx.delay.delay_ms(10);
    }
    
    logger.info("Scan complete");
}

pub fn forever(ctx: &mut Context) {
    ctx.delay.delay_ms(1000);
}

