const wokwi = @import("wokwi-api.zig");

const Chip = extern struct {
    i2c_dev: wokwi.I2CDevId,
};

// Called when the I2C master addresses this chip
export fn chip_i2c_connect(user_data: ?*anyopaque, address: u32, connect_type: u32) bool {
    _ = user_data;
    _ = connect_type;

    // We only respond to address 0x42
    if (address == 0x42) {
        wokwi.debugPrint("I2C Connect: Address 0x42 matched!");
        return true; // ACK
    }
    return false; // NACK
}

// Called when the master reads from the chip
export fn chip_i2c_read(user_data: ?*anyopaque) u8 {
    _ = user_data;
    // Return a dummy value (e.g., 0xAB)
    return 0xAB;
}

// Called when the master writes to the chip
export fn chip_i2c_write(user_data: ?*anyopaque, data: u8) bool {
    _ = user_data;
    _ = data;
    // Check data if needed, return true to ACK
    return true;
}

// Called when the transaction ends
export fn chip_i2c_disconnect(user_data: ?*anyopaque) void {
    _ = user_data;
}

export fn chipInit() callconv(.c) void {
    wokwi.debugPrint("Zig I2C Custom Chip Init (Address: 0x42)");

    var chip: Chip = .{
        .i2c_dev = 0,
    };

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
