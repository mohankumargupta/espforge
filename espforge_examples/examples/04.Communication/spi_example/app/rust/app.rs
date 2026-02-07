#[warn(unused_variables)]

use crate::{component, Context};
use embedded_hal::spi::SpiBus;

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    let spi = component!(my_spi);
    
    logger.info("SPI Example");
    
    // Example: Transfer data (Write 0x03, Read next byte)
    // In full-duplex SPI, read and write happen simultaneously.
    let mut buffer = [0x03, 0x00];
    
    match spi.transfer_in_place(&mut buffer) {
        Ok(_) => {
            logger.info("Sent 0x03");
            logger.info(format_args!("Received: {}", buffer[1]));
        }
        Err(_) => {
            logger.info("SPI Transfer failed");
        }
    }
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;
    delay.delay_ms(1000);
}

