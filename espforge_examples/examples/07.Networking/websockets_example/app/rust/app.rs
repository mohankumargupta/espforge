use crate::{ Context, component, Message };
use embassy_executor::Spawner;

pub async fn setup(ctx: &mut Context,  _spawner: Spawner) {
    let logger = ctx.logger;
    logger.info("Starting WebSocket example");
}

pub async fn forever(ctx: &mut Context) {
    let ws = component!(ws_client);
    
    // Connect
    match ws.connect().await {
        Ok(_) => ctx.logger.info("WebSocket connected!"),
        Err(e) => {
                    ctx.logger.info(format_args!("WebSocket error: {:?}", e));
        panic!("connect failed");
        }
    }
    
    // Send message
    ws.send_text("Hello, WebSocket!").await.unwrap();
    
    // Receive (non-blocking pattern)
    let mut buf = [0u8; 1024];
    if let Some(msg) = ws.receive(&mut buf).await.unwrap() {
        match msg {
            Message::Text(s) => ctx.logger.info(s),
            Message::Binary(_d) => { /* handle binary */ },
            Message::Close => { /* handle close */ },
            _ => {}
        }
    }
}

