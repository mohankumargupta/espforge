//const std = @import("std");
const wokwi = @import("wokwi-api.zig");

// Constants
const REGMAP_SIZE = 6;

// // Struct to hold chip state
const Chip = extern struct {
    spi: wokwi.SPIDevId,
    cs_pin: wokwi.PinId, // Added CS Pin ID
    // We use a specific array of 1 for the buffer, similar to the C struct
    spi_buffer: [1]u8,
    regmap: [REGMAP_SIZE]u8,
    current_index: u8,
};

// Global instance

// // -------------------------------------------------------------------------
// // CS Pin Change Callback
// // -------------------------------------------------------------------------

export fn chip_pin_change(user_data: ?*anyopaque, pin: wokwi.PinId, value: u32) callconv(.c) void {
    _ = pin; // Unused
    //_ = user_data;
    //_ = value;
    const chip: *Chip = @ptrCast(@alignCast(user_data.?));

    if (value == wokwi.LOW) {
        //wokwi.debugPrint("CS pulled low");
        //         // CS went LOW: Start the SPI transaction

        //         // Optional: Reset buffer to a known state (e.g., 0x00) or current register value
        //         // before the first clock cycle arrives.
        chip.spi_buffer[0] = 0x00;

        const buffer_ptr: [*:0]const u8 = @ptrCast(&chip.spi_buffer);
        wokwi.spiStart(chip.spi, buffer_ptr, 1);
    } else {
        //wokwi.debugPrint("CS pulled high");
        //         // CS went HIGH: Stop the SPI transaction
        wokwi.spiStop(chip.spi);

        //         // Optional: specific logic for CS high (e.g., reset state machine)
        chip.current_index = 0;
    }
}

// // -------------------------------------------------------------------------
// // SPI Callback
// // -------------------------------------------------------------------------

export fn chip_spi_done(user_data: ?*anyopaque, buffer_ptr: [*]u8, count: u32) callconv(.c) void {
    _ = count; // Unused, we know it is 1
    //_ = user_data;
    //_ = buffer_ptr;
    const chip: *Chip = @ptrCast(@alignCast(user_data.?));

    //     // 'buffer[0]' holds the byte we JUST received
    const received_cmd = buffer_ptr[0];

    //     // --- LOGIC ---
    if (received_cmd != 0) {
        //         // CASE A: Received an Index/Address
        if (received_cmd < REGMAP_SIZE) {
            wokwi.debugPrint("register write index command received.");
            chip.current_index = received_cmd;
        } else {
            //             // Out of bounds
            chip.current_index = 0;
        }
    } else {
        //         // CASE B: Received 0x00 (Read Command)
        //         // Keep index same
        //         //wokwi.debugPrint("Read command received.");
    }

    //     // --- PREPARE RESPONSE FOR NEXT BYTE ---

    //     // Load buffer with value at current index so it shifts out on next byte
    buffer_ptr[0] = chip.regmap[chip.current_index];

    //     // Restart SPI transaction for the next byte.
    //     // NOTE: If CS goes high immediately after this, spiStop() in chip_pin_change
    //     // will cancel this pending start.
    const buffer_casted: [*:0]const u8 = @ptrCast(buffer_ptr);
    wokwi.spiStart(chip.spi, buffer_casted, 1);
}

// -------------------------------------------------------------------------
// Chip Initialization
// -------------------------------------------------------------------------

export fn chipInit() callconv(.c) void {
    wokwi.debugPrint("Zig SPI Chip Init (with CS)");

    var chip: Chip = .{
        .spi = 0,
        .cs_pin = 0,
        .spi_buffer = .{0},
        .regmap = .{0} ** REGMAP_SIZE,
        .current_index = 0,
    };

    // // 1. Setup data
    chip.regmap[0] = 0;
    chip.regmap[1] = 10;
    chip.regmap[2] = 20;
    chip.regmap[3] = 30;
    chip.regmap[4] = 40;
    chip.regmap[5] = 50;
    chip.current_index = 0;

    // // 2. Configure SPI
    const spi_config = wokwi.SPIConfig{
        .user_data = @ptrCast(&chip),
        .sck = wokwi.pinInit("SCK", wokwi.INPUT),
        .mosi = wokwi.pinInit("MOSI", wokwi.INPUT),
        .miso = wokwi.pinInit("MISO", wokwi.INPUT),
        .mode = 0,
        .done = @constCast(&chip_spi_done),
    };
    chip.spi = wokwi.spiInit(@constCast(&spi_config));

    // // 3. Configure CS Pin and Watcher
    chip.cs_pin = wokwi.pinInit("CS", wokwi.INPUT);

    const pin_watch_config = wokwi.WatchConfig{
        .user_data = @ptrCast(&chip),
        .edge = wokwi.BOTH, // Listen for both Rising and Falling edges
        .pin_change = @constCast(&chip_pin_change),
    };

    // // Start watching the CS pin
    _ = wokwi.pinWatch(chip.cs_pin, @constCast(&pin_watch_config));

    // // NOTE: We do NOT call spiStart here anymore.
    // // We wait for the CS pin to go LOW in chip_pin_change.
}
