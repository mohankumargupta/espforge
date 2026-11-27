use crate::{service, Context};
use embassy_executor::Spawner;

/// Runs once at startup: wait for Wi-Fi, then say what we are about to do.
pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    ctx.logger.info("Waiting for Wi-Fi connection...");
    service!(ctx, home).wait_connected().await;
    ctx.logger.info("Connected. Streaming status over secure WebSocket.");
}

/// Per-tick loop: open a TLS session, send one JSON frame, read the echo, sleep.
pub async fn forever(ctx: &'static Context) {
    let wss = service!(ctx, secure_ws);
    // Socket buffers first, then the two TLS record buffers, all borrowed for
    // the life of the session.
    let mut conn_buf = [0u8; 8192];
    let mut rx_buf = [0u8; 512];
    let payload = r#"{"sensor":"temp","value":24.5}"#;

    // Not `websocket-echo.com` on 443: it aborts embedded-tls's TLS 1.3
    // handshake with a fatal `handshake_failure`. `echo.websocket.events` no
    // longer resolves at all, so the client reports a DNS error.
    match wss
        .connect("wss://echo.websocket.org/", &mut conn_buf)
        .await
    {
        Ok(mut session) => {
            ctx.logger.info("TLS WebSocket open. Sending telemetry...");
            if let Err(e) = session.send_text(payload).await {
                ctx.logger.warn(format_args!("send failed: {e:?}"));
            }

            // This endpoint greets every session with one text frame of its
            // own ("Request served by <node>", 32 bytes) before echoing, so
            // the first frame received is not the echo. Read a few frames and
            // stop on the one that carries the payload back.
            let mut echoed = false;
            for _ in 0..4 {
                match session.recv(&mut rx_buf).await {
                    Ok(msg) => match core::str::from_utf8(msg.payload) {
                        Ok(text) if text == payload => {
                            ctx.logger.info(format_args!("Echo: {text}"));
                            echoed = true;
                            break;
                        }
                        Ok(text) => ctx.logger.info(format_args!("Server: {text}")),
                        Err(_) => ctx.logger.warn(format_args!(
                            "Server: {} bytes (not utf-8)",
                            msg.payload.len()
                        )),
                    },
                    Err(e) => {
                        ctx.logger.warn(format_args!("stream error: {e:?}"));
                        break;
                    }
                }
            }

            if !echoed {
                ctx.logger.warn("No echo within four frames");
            }

            let _ = session.close(1000).await;
        }
        Err(err) => ctx
            .logger
            .error(format_args!("WSS connection failed: {err:?}")),
    }

    ctx.delay.delay_ms(5000).await;
}
