const wokwi = @import("wokwi-api.zig");

// Constants
const REGMAP_SIZE = 6;

// Struct to hold chip state
const Chip = extern struct {
    i2c_dev: wokwi.I2CDevId,
    regmap: [REGMAP_SIZE]u8,
    current_index: u8,
    register_written: bool,
};

// -------------------------------------------------------------------------
// I2C Connect Callback
// -------------------------------------------------------------------------
// Called when the I2C master addresses this chip. Return true to ACK.
export fn chip_i2c_connect(user_data: ?*anyopaque, address: u32, connect_type: u32) bool {
    _ = connect_type;

    if (address != 0x42) return false; // NACK

    const chip: *Chip = @ptrCast(@alignCast(user_data.?));
    // Reset transaction state at the start of each new transaction
    chip.current_index = 0;
    chip.register_written = false;
    return true; // ACK
}

// -------------------------------------------------------------------------
// I2C Read Callback
// -------------------------------------------------------------------------
// Called when the master reads a byte from this chip.
// Returns the value at current_index, then auto-increments the index
// (wrapping within the regmap), mirroring the SPI chip's behavior where
// each read shifts out regmap[current_index].
export fn chip_i2c_read(user_data: ?*anyopaque) u8 {
    const chip: *Chip = @ptrCast(@alignCast(user_data.?));
    const value = chip.regmap[chip.current_index];
    // Auto-increment for sequential reads (wrapping)
    chip.current_index = (chip.current_index + 1) % REGMAP_SIZE;
    return value;
}

// -------------------------------------------------------------------------
// I2C Write Callback
// -------------------------------------------------------------------------
// Called when the master writes a byte to this chip.
// The first byte of a write transaction sets the register index.
// Additional bytes are ignored (for simplicity).
export fn chip_i2c_write(user_data: ?*anyopaque, data: u8) bool {
    const chip: *Chip = @ptrCast(@alignCast(user_data.?));
    // First write byte of the transaction sets the register index.
    // Subsequent bytes in the same transaction are ignored.
    if (!chip.register_written and data < REGMAP_SIZE) {
        chip.current_index = data;
        chip.register_written = true;
    }
    return true; // ACK
}

// -------------------------------------------------------------------------
// I2C Disconnect Callback
// -------------------------------------------------------------------------
export fn chip_i2c_disconnect(user_data: ?*anyopaque) void {
    _ = user_data;
}

// -------------------------------------------------------------------------
// Chip Initialization
// -------------------------------------------------------------------------
export fn chipInit() callconv(.c) void {
    wokwi.debugPrint("Zig I2C Custom Chip Init (Address: 0x42)");

    var chip: Chip = .{
        .i2c_dev = 0,
        .regmap = .{0} ** REGMAP_SIZE,
        .current_index = 0,
        .register_written = false,
    };

    // Setup register map — same values as the SPI chip
    chip.regmap[0] = 0;
    chip.regmap[1] = 10;
    chip.regmap[2] = 20;
    chip.regmap[3] = 30;
    chip.regmap[4] = 40;
    chip.regmap[5] = 50;
    chip.current_index = 0;

    // Configure I2C
    const i2c_config = wokwi.I2CConfig{
        .user_data = @ptrCast(&chip),
        .address = 0x42,
        .scl = wokwi.pinInit("SCL", wokwi.INPUT),
        .sda = wokwi.pinInit("SDA", wokwi.INPUT),
        .connect = @constCast(&chip_i2c_connect),
        .read = @constCast(&chip_i2c_read),
        .write = @constCast(&chip_i2c_write),
        .disconnect = @constCast(&chip_i2c_disconnect),
    };

    chip.i2c_dev = wokwi.i2cInit(@constCast(&i2c_config));
}
