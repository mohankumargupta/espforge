use crate::{component, Context};
use embedded_hal::i2c::I2c;

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    let delay = ctx.delay;
    let i2c = component!(my_i2c);

    logger.info("I2C Scanner Example");
    
    // Scan addresses 1 to 127
    for address in 1..127 {
        // Try to write a 0-byte message to the address
        // If it ACKs, there is a device there
        match i2c.write(address, &[]) {
            Ok(_) => {
                logger.info(format_args!("Found device at address 0x{:02x}", address));
            },
            Err(_) => {
                // No device or error
            }
        }
        
        // Small delay between scans not to flood
        delay.delay_ms(10);
    }
    
    ctx.logger.info("Scan complete");
}

pub fn forever(ctx: &mut Context) {
    ctx.delay.delay_ms(1000);
}