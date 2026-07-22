const std = @import("std");

pub fn build(b: *std.Build) void {
    const exe = b.addExecutable(.{
        .name = "chip",
        .root_module = b.createModule(.{
            .root_source_file = b.path("chip.zig"),
            .target = b.resolveTargetQuery(.{
                .cpu_arch = .wasm32,
                .os_tag = .freestanding,
            }),
            .optimize = .ReleaseFast,
        }),
    });

    exe.export_table = true;
    exe.rdynamic = true;
    exe.entry = .disabled;

    const install_step = b.addUpdateSourceFiles();
    install_step.addCopyFileToSource(exe.getEmittedBin(), "../chip.wasm");
    b.getInstallStep().dependOn(&install_step.step);
}
