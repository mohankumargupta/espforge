#![allow(unused_variables)]

use crate::embedded_hal::spi::Operation;

use crate::Context;

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    let spi = device!(ctx, spi2);

    logger.info("SPI Example");

    // The raw SPI device owns the chip-select line declared by `<spi cs="9">`.
    // Full-duplex SPI reads and writes simultaneously, so we use
    // `TransferInPlace` to send 0x03 and read the response in one transaction.
    let mut buffer = [0x03u8, 0x00];
    match spi.transaction(&mut [Operation::TransferInPlace(&mut buffer)]) {
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
