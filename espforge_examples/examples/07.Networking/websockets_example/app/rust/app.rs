use crate::{Context, component};
use embassy_executor::Spawner;
use embassy_time::{with_timeout, Duration};
use espforge_components::Message;

pub async fn setup(ctx: &mut Context, _spawner: Spawner) {
    ctx.logger.info("Starting WebSocket example");
    let mut ws = component!(ws_client);
    let uri = "wss://echo.websocket.org";

    match ws.connect(uri).await {
        Ok(mut session) => {
            ctx.logger.info("WebSocket connected!");

            let mut greeting_buf = [0u8; 256];
            if let Ok(Message::Text(s)) = session.receive(&mut greeting_buf).await {
                ctx.logger.info(format_args!("Greeting: {}", s));
            }

            for payload in ["Hello world!", "How are you?", "I'm fine, thanks!"] {
                ctx.logger.info(format_args!("Sending: {}", payload));

                ctx.delay.delay_ms(1000).await;
                

                match with_timeout(Duration::from_secs(5), session.send_text(payload)).await {
                    Ok(Ok(())) => ctx.logger.info("Send completed"),
                    Ok(Err(e)) => {
                        ctx.logger.info(format_args!("Send failed: {:?}", e));
                        break;
                    }
                    Err(_) => {
                        ctx.logger.info("Send TIMED OUT (stuck in write/flush)");
                        break;
                    }
                }

                let mut buf = [0u8; 1024];
                ctx.logger.info("Waiting for reply...");
                match with_timeout(Duration::from_secs(5), session.receive(&mut buf)).await {
                    Ok(Ok(Message::Text(s))) => ctx.logger.info(format_args!("Got text: {}", s)),
                    Ok(Ok(Message::Binary(_))) => ctx.logger.info("Got binary data"),
                    Ok(Ok(Message::Close)) => {
                        ctx.logger.info("Server closed the connection");
                        break;
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => {
                        ctx.logger.info(format_args!("Receive error: {:?}", e));
                        break;
                    }
                    Err(_) => {
                        ctx.logger.info("Receive TIMED OUT (stuck waiting for reply)");
                        break;
                    }
                }
            }

            ctx.logger.info("Closing connection");
            let _ = session.close().await;
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

