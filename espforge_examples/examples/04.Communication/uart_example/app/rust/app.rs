#[allow(unused_variables)]

use crate::{component, Context};

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    let uart = component!(my_uart); 
    
    logger.info("UART Example");
    uart.write("Hello\n");
}

pub fn forever(ctx: &mut Context) {
    let logger = ctx.logger;
    let delay = ctx.delay;
    let uart = component!(my_uart);
    
    if uart.buffer_until_newline() {        
        logger.info("Message received:");
        logger.info(uart.get_buffered_string());
        uart.clear_buffer();
    }
    
    delay.delay_ms(10);
}
