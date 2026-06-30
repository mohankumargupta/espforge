use crate::{ Context, component, Message };
use embassy_executor::Spawner;

pub async fn setup(ctx: &mut Context,  _spawner: Spawner) {
    let logger = ctx.logger;
    logger.info("Starting WebSocket example");
}

pub async fn forever(ctx: &mut Context) {
    let ws = component!(ws_client);

    ctx.logger.info(format_args!("has_tls: {}", ws.has_tls()));
    
    // Connect
    match ws.connect().await {
        Ok(_) => ctx.logger.info("WebSocket connected!"),
        Err(e) => {
            ctx.logger.info(format_args!("WebSocket error: {:?}", e));
            panic!("connect failed");
        }
    }
    
    for payload in ["Hello world!", "How are you?", "I'm fine, thanks!"] {
        ctx.logger.info(format_args!("Sending: {}", payload));
        ws.send_text(payload).await.unwrap();

        let mut buf = [0u8; 1024];
        if let Some(msg) = ws.receive(&mut buf).await.unwrap() {
            match msg {
                Message::Text(s) => ctx.logger.info(format_args!("Got text: {}", s)),
                Message::Binary(_d) => ctx.logger.info("Got binary data"),
                Message::Close => ctx.logger.info("Got close"),
                _ => {}
            }
        }
    }

    ctx.logger.info("Closing connection");
    ws.close().await.unwrap();

    ctx.delay.delay_ms(10000).await;
}

