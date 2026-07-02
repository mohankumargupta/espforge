use crate::{Context, component};
use embassy_executor::Spawner;
use espforge_components::Message;

pub async fn setup(ctx: &mut Context, _spawner: Spawner) {
    ctx.logger.info("Starting WebSocket example");
    let mut ws = component!(ws_client);
    let uri = "wss://echo.websocket.org";

    match ws.connect(uri).await {
        Ok(mut session) => {
            ctx.logger.info("WebSocket connected!");
            for payload in ["Hello world!", "How are you?", "I'm fine, thanks!"] {
                ctx.logger.info(format_args!("Sending: {}", payload));
                session.send_text(payload).await.unwrap();

                let mut buf = [0u8; 1024];
                match session.receive(&mut buf).await {
                    Ok(msg) => match msg {
                        Message::Text(s) => ctx.logger.info(format_args!("Got text: {}", s)),
                        Message::Binary(_) => ctx.logger.info("Got binary data"),
                        Message::Close => ctx.logger.info("Got close"),
                        _ => {}
                    },
                    Err(e) => {
                        ctx.logger.info(format_args!("Receive error: {:?}", e));
                        break;
                    }
                }
            }
            ctx.logger.info("Closing connection");
            session.close().await.unwrap();
        }
        Err(e) => {
            ctx.logger.info(format_args!("WebSocket error: {:?}", e));
            panic!("connect failed");
        }
    }
}

pub async fn forever(ctx: &mut Context) {
    ctx.delay.delay_ms(1000).await;
}