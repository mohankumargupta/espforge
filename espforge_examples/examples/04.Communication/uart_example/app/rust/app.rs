#[warn(unused_variables)]

use crate::Context;

pub fn setup(ctx: &mut Context) {
    ctx.logger.info("UART Example");
    // Access component by mutable reference to avoid move
    let uart = &mut ctx.components.my_uart; 
    uart.write("Hello\n");
}

pub fn forever(ctx: &mut Context) {
    let uart = &mut ctx.components.my_uart;
    
    // Check for buffered line (non-blocking)
    if uart.buffer_until_newline() {        
        ctx.logger.info("Message received:");
        ctx.logger.info(uart.get_buffered_string());
        uart.clear_buffer();
    }
    
    // Use delay from context
    ctx.delay.delay_ms(10);
}
