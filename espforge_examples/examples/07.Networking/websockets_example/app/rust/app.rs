use crate::{Context, component};
use embassy_executor::Spawner;
use espforge_components::Message;
use espforge_components::components::websockets::{
    SessionContext, Tls, TlsRng, WebSocketConnector, WebSocketError,
};

pub async fn setup(ctx: &mut Context, _spawner: Spawner) {
    ctx.logger.info("Starting WebSocket example");
}

pub async fn forever(ctx: &mut Context) {
    let mut connector = component!(ws_client);
    let uri = "wss://ws.postman-echo.com/raw";
    let uses_tls = uri.starts_with("wss://");
    ctx.logger.info(format_args!("Using TLS: {}", uses_tls));

    // RNG must outlive `tls`, which must outlive `tls_ref`/`session_ctx`/`session`.
    let mut rng = TlsRng::new(unsafe { espforge_platform::rng::Rng::new() });
    let tls = unsafe { Tls::new_local_borrows(&mut rng) }
        .map_err(|_| WebSocketError::TlsError)
        .expect("tls init failed");

    let tls_ref = if uses_tls { Some(tls.reference()) } else { None };

    let mut session_ctx = SessionContext {
        tls: None,
        plain_tcp: None,
    };

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

