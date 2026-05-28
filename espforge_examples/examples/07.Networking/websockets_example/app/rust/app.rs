use crate::{ Context, component };
use embassy_executor::Spawner;

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    logger.info("Starting WebSocket example");
}

pub async fn forever(ctx: &mut Context) {
    let ws = component!(ws_client);
    
    // Connect
    ws.connect().await.unwrap();
    
    // Send message
    ws.send_text("Hello, WebSocket!").await.unwrap();
    
    // Receive (non-blocking pattern)
    let mut buf = [0u8; 1024];
    if let Some(msg) = ws.receive(&mut buf).await.unwrap() {
        match msg {
            Message::Text(s) => ctx.logger.info(s),
            Message::Binary(d) => { /* handle binary */ },
            Message::Close(_) => { /* handle close */ },
            _ => {}
        }
    }
}

