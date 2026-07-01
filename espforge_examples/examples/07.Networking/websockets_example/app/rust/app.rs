// Correct imports
use crate::{Context, component};
use embassy_executor::Spawner;
use espforge_components::Message; // as suggested by the compiler
use espforge_components::components::websockets::{
    SessionContext, WebSocketConnector, WebSocketError,
};

pub async fn setup(ctx: &mut Context, _spawner: Spawner) {
    let logger = ctx.logger;
    logger.info("Starting WebSocket example");
}

pub async fn forever(ctx: &mut Context) {
    // Get the connector component (it's a factory)
    let mut connector = component!(ws_client);

    // Determine if we are using TLS (just for logging)
    let uri = "wss://echo.websocket.org"; // change to your server
    let uses_tls = uri.starts_with("wss://");
    ctx.logger.info(format_args!("Using TLS: {}", uses_tls));

    // Create a session context – this will own the TCP/TLS transport
    // and must live as long as the session.
    let mut session_ctx = SessionContext {
        tls: None, // set to Some(TlsReference) if using wss://
        plain_tcp: None,
    };

    // For plain WS, pass `None` for TLS reference.
    let tls_ref = None;

    // Connect – this yields a WebSocketSession
    match connector.connect(uri, tls_ref, &mut session_ctx).await {
        Ok(mut session) => {
            ctx.logger.info("WebSocket connected!");

            for payload in ["Hello world!", "How are you?", "I'm fine, thanks!"] {
                ctx.logger.info(format_args!("Sending: {}", payload));
                session.send_text(payload).await.unwrap();

                let mut buf = [0u8; 1024];
                match session.receive(&mut buf).await {
                    Ok(msg) => match msg {
                        Message::Text(s) => ctx.logger.info(format_args!("Got text: {}", s)),
                        Message::Binary(_d) => ctx.logger.info("Got binary data"),
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

    ctx.delay.delay_ms(10000).await;
}
