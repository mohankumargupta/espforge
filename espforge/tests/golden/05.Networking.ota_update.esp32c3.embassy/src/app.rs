//! A/B firmware updates over Wi-Fi: `ctx.ota` plus a streaming GET.
//!
//! The running image confirms itself only after Wi-Fi works, then asks the
//! server what it has and streams it into the inactive slot. When this file is
//! v1, the server's `firmware.bin` is v2; after `finish()` and `reboot()` the
//! bootloader starts v2.

use crate::{service, Context};
use embassy_executor::Spawner;

/// The update server, serving `manifest.txt` and `firmware.bin`.
const MANIFEST_URL: &str = "http://192.168.1.50:8000/manifest.txt";
const FIRMWARE_URL: &str = "http://192.168.1.50:8000/firmware.bin";

/// The manifest's two lines: `size=<bytes>` and `crc32=<hex or decimal>`.
///
/// This is the server's contract, not espforge's: any way of learning the
/// image's length and CRC32 works, as long as they describe exactly the bytes
/// `get_stream` will hand to `ctx.ota.write`.
async fn fetch_manifest(ctx: &'static Context, url: &str) -> Result<(u32, u32), &'static str> {
    let mut body = [0u8; 128];
    let body = service!(ctx, http)
        .get(url, &mut body)
        .await
        .map_err(|_| "manifest request failed")?;
    let text = core::str::from_utf8(body).map_err(|_| "manifest is not UTF-8")?;

    let mut size = None;
    let mut crc32 = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("size=") {
            size = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("crc32=") {
            let value = value.trim();
            crc32 = match value.strip_prefix("0x") {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => value.parse().ok(),
            };
        }
    }
    match (size, crc32) {
        (Some(size), Some(crc32)) => Ok((size, crc32)),
        _ => Err("manifest has no size/crc32"),
    }
}

/// `begin` → stream `write` → `finish` → `reboot`.
async fn apply(
    ctx: &'static Context,
    url: &str,
    image_size: u32,
    target_crc32: u32,
) -> Result<(), &'static str> {
    ctx.ota
        .begin(image_size, target_crc32)
        .map_err(|_| "ota begin failed")?;

    // `write` returns true once the whole image is in flash. `get_stream`
    // hands over 2 KiB chunks, so the image never has to fit in RAM.
    let mut done = false;
    service!(ctx, http)
        .get_stream(url, |chunk| {
            done = ctx.ota.write(chunk).map_err(|_| ())?;
            if let Some(percent) = ctx.ota.progress() {
                ctx.logger.info(format_args!("OTA progress: {percent}%"));
            }
            Ok::<(), ()>(())
        })
        .await
        .map_err(|_| "download/write failed")?;

    if !done {
        return Err("incomplete firmware download");
    }

    // Verifies CRC32 and reads the image back from flash before activating it.
    ctx.ota.finish().map_err(|_| "ota verify/finish failed")?;

    ctx.logger.info("OTA verified; rebooting into the new slot");
    ctx.ota.reboot();
}

pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    ctx.logger.info("OTA update example");

    service!(ctx, home).wait_connected().await;
    ctx.logger.info("Wi-Fi connected");

    // Prove this image can connect and take future updates before confirming
    // its slot: rollback reverts on the next reset if this never runs.
    if let Err(error) = ctx.ota.mark_app_valid() {
        ctx.logger
            .warn(format_args!("mark_app_valid failed: {error}"));
    }

    let (image_size, target_crc32) = match fetch_manifest(ctx, MANIFEST_URL).await {
        Ok(manifest) => manifest,
        Err(error) => {
            ctx.logger.warn(format_args!("no update: {error}"));
            return;
        }
    };

    if let Err(error) = apply(ctx, FIRMWARE_URL, image_size, target_crc32).await {
        ctx.logger.error(format_args!("OTA failed: {error}"));
    }
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(10_000).await;
}
