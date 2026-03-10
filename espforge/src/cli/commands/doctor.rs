use anyhow::Result;
use std::process::Command;

// Assumes latest esp-generate version works in espforge 


/// All supported chips with their architecture and RISC-V target (if applicable).
const CHIPS: &[ChipInfo] = &[
    ChipInfo { name: "ESP32",    arch: Arch::Xtensa, riscv_target: None },
    ChipInfo { name: "ESP32-S2", arch: Arch::Xtensa, riscv_target: None },
    ChipInfo { name: "ESP32-S3", arch: Arch::Xtensa, riscv_target: None },
    ChipInfo { name: "ESP32-C2", arch: Arch::RiscV,  riscv_target: Some("riscv32imc-unknown-none-elf") },
    ChipInfo { name: "ESP32-C3", arch: Arch::RiscV,  riscv_target: Some("riscv32imc-unknown-none-elf") },
    ChipInfo { name: "ESP32-C6", arch: Arch::RiscV,  riscv_target: Some("riscv32imac-unknown-none-elf") },
    ChipInfo { name: "ESP32-H2", arch: Arch::RiscV,  riscv_target: Some("riscv32imac-unknown-none-elf") },
];

struct ChipInfo {
    name: &'static str,
    arch: Arch,
    riscv_target: Option<&'static str>,
}

#[derive(PartialEq)]
enum Arch {
    Xtensa,
    RiscV,
}

struct CheckResult {
    name: &'static str,
    status: Status,
    version: Option<String>,
    note: Option<&'static str>,
}

enum Status {
    Ok,
    Warning(String),
    Missing,
}

/// Snapshot of environment facts, used both for individual checks and the chip summary.
struct EnvState {
    has_cargo: bool,
    has_esp_toolchain: bool,  // 'esp' rustup channel — required for Xtensa
    has_stable_toolchain: bool,
    has_gcc_toolchain: bool,  // xtensa-esp-elf-gcc — required for Xtensa linking
    has_esp_generate: bool,
    installed_riscv_targets: Vec<String>,
}

pub fn execute() -> Result<()> {
    println!();
    println!("🔬 espforge doctor — environment check");
    println!("{}", "─".repeat(50));
    println!();

    let env = probe_environment();

    let checks = vec![
        check_cargo(&env),
        check_esp_toolchain(&env),
        check_esp_generate(&env),
        check_gcc_toolchain(&env),
        check_riscv_targets(&env),
    ];

    let mut all_ok = true;

    for check in &checks {
        let icon = match &check.status {
            Status::Ok => "✅",
            Status::Warning(_) => "⚠️ ",
            Status::Missing => "❌",
        };

        let version_str = check
            .version
            .as_deref()
            .map(|v| format!("  ({})", v))
            .unwrap_or_default();

        println!("  {} {}{}", icon, check.name, version_str);

        match &check.status {
            Status::Ok => {}
            Status::Warning(msg) => {
                println!("       {}", msg);
                all_ok = false;
            }
            Status::Missing => {
                all_ok = false;
            }
        }

        if let Some(note) = check.note {
            println!("       ℹ️  {}", note);
        }
    }

    println!();
    if all_ok {
        println!("✨ All checks passed — you're ready to use espforge!");
    } else {
        println!("⚡ Some issues were found. See above for details.");
        println!();
        println!("  Quick fix:");
        println!("    cargo install cargo-binstall");
        println!("    cargo binstall espup");
        println!("    espup install");
        println!("    cargo binstall esp-generate");
        println!("    rustup target add riscv32imc-unknown-none-elf");
        println!("    rustup target add riscv32imac-unknown-none-elf");
    }

    print_chip_summary(&env);

    Ok(())
}

/// Probe the environment once and return a shared state struct used by all checks.
fn probe_environment() -> EnvState {
    let has_cargo = run_version_cmd("cargo", &["--version"]).is_some();
    let has_gcc_toolchain = run_version_cmd("xtensa-esp-elf-gcc", &["--version"]).is_some();
    let has_esp_generate = run_version_cmd("esp-generate", &["--version"]).is_some();

    let toolchain_list = Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let has_esp_toolchain = toolchain_list.lines().any(|l| l.starts_with("esp"));
    let has_stable_toolchain = toolchain_list.lines().any(|l| l.starts_with("stable"));

    let installed_riscv_targets = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .collect()
        })
        .unwrap_or_default();

    EnvState {
        has_cargo,
        has_esp_toolchain,
        has_stable_toolchain,
        has_gcc_toolchain,
        has_esp_generate,
        installed_riscv_targets,
    }
}

fn print_chip_summary(env: &EnvState) {
    println!();
    println!("📋 Chip readiness summary");
    println!("{}", "─".repeat(50));
    println!();

    let name_w = 10;
    let arch_w = 9;

    println!(
        "  {:<name_w$}  {:<arch_w$}  {}",
        "Chip", "Arch", "Status",
        name_w = name_w,
        arch_w = arch_w,
    );
    println!("  {}", "─".repeat(46));

    for chip in CHIPS {
        let (icon, reason) = chip_status(chip, env);
        let arch_label = match chip.arch {
            Arch::Xtensa => "Xtensa",
            Arch::RiscV  => "RISC-V",
        };
        println!(
            "  {:<name_w$}  {:<arch_w$}  {} {}",
            chip.name,
            arch_label,
            icon,
            reason,
            name_w = name_w,
            arch_w = arch_w,
        );
    }

    println!();
}

/// Returns (icon, human-readable status) for a single chip.
fn chip_status(chip: &ChipInfo, env: &EnvState) -> (&'static str, String) {
    let base_ok = env.has_cargo && env.has_esp_generate;

    match chip.arch {
        Arch::Xtensa => {
            let mut missing: Vec<&str> = Vec::new();
            if !base_ok                { missing.push("cargo / esp-generate") }
            if !env.has_esp_toolchain  { missing.push("esp toolchain (espup install)") }
            if !env.has_gcc_toolchain  { missing.push("xtensa-esp-elf-gcc (espup install)") }

            if missing.is_empty() {
                ("✅", "Ready".to_string())
            } else {
                ("❌", format!("Missing: {}", missing.join(", ")))
            }
        }

        Arch::RiscV => {
            let target = chip.riscv_target.unwrap_or("");
            let has_target = env.installed_riscv_targets.iter().any(|t| t == target);
            let has_toolchain = env.has_stable_toolchain || env.has_esp_toolchain;

            let mut missing: Vec<String> = Vec::new();
            if !base_ok       { missing.push("cargo / esp-generate".to_string()) }
            if !has_toolchain { missing.push("stable toolchain (rustup toolchain install stable)".to_string()) }
            if !has_target    { missing.push(format!("{} (rustup target add {})", target, target)) }

            if missing.is_empty() {
                ("✅", "Ready".to_string())
            } else {
                ("❌", format!("Missing: {}", missing.join(", ")))
            }
        }
    }
}

fn check_cargo(env: &EnvState) -> CheckResult {
    if env.has_cargo {
        CheckResult {
            name: "cargo / Rust",
            status: Status::Ok,
            version: run_version_cmd("cargo", &["--version"]),
            note: None,
        }
    } else {
        CheckResult {
            name: "cargo / Rust",
            status: Status::Missing,
            version: None,
            note: Some("Install Rust from https://rustup.rs/"),
        }
    }
}

fn check_esp_toolchain(env: &EnvState) -> CheckResult {
    // The ESP toolchain (for Xtensa chips like esp32, esp32s3) is installed via `espup`
    // and shows up as the "esp" channel in `rustup toolchain list`.
    // RISC-V chips (esp32c3, esp32c6, etc.) use the standard stable toolchain.
    if env.has_esp_toolchain {
        let esp_version = Command::new("rustup")
            .args(["toolchain", "list"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .find(|l| l.starts_with("esp"))
                    .map(|l| l.trim().to_string())
            });

        CheckResult {
            name: "ESP toolchain (Xtensa/RISC-V via espup)",
            status: Status::Ok,
            version: esp_version,
            note: Some("'esp' channel required for Xtensa chips (esp32, esp32s3, esp32s2)"),
        }
    } else if env.has_stable_toolchain {
        CheckResult {
            name: "ESP toolchain (Xtensa/RISC-V via espup)",
            status: Status::Warning(
                "Only 'stable' toolchain found. Xtensa chips (esp32, esp32s3) require \
                 the 'esp' channel.\n       Run: espup install"
                    .to_string(),
            ),
            version: None,
            note: Some("RISC-V chips (esp32c3, esp32c6, etc.) work with stable"),
        }
    } else {
        CheckResult {
            name: "ESP toolchain (Xtensa/RISC-V via espup)",
            status: Status::Missing,
            version: None,
            note: Some("Run: cargo binstall espup && espup install"),
        }
    }
}

fn check_esp_generate(env: &EnvState) -> CheckResult {
    if !env.has_esp_generate {
        return CheckResult {
            name: "esp-generate",
            status: Status::Missing,
            version: None,
            note: Some("Run: cargo binstall esp-generate"),
        };
    }

    match run_version_cmd("esp-generate", &["--version"]) {
        None => CheckResult {
            name: "esp-generate",
            status: Status::Missing,
            version: None,
            note: Some("Run: cargo binstall esp-generate"),
        },
        Some(raw_version) => {
            let semver = raw_version
                .split_whitespace()
                .last()
                .unwrap_or(&raw_version)
                .to_string();

            let status = Status::Ok;

            CheckResult {
                name: "esp-generate",
                status,
                version: Some(semver),
                note: Some(
                    "",
                ),
            }
        }
    }
}

fn check_gcc_toolchain(env: &EnvState) -> CheckResult {
    // espup installs the Xtensa GCC toolchain as `xtensa-esp-elf-gcc`, used to link
    // the final binary for Xtensa targets (esp32, esp32s2, esp32s3).
    if env.has_gcc_toolchain {
        CheckResult {
            name: "GCC toolchain (xtensa-esp-elf-gcc)",
            status: Status::Ok,
            version: run_version_cmd("xtensa-esp-elf-gcc", &["--version"]),
            note: Some("Required for linking Xtensa binaries (esp32, esp32s2, esp32s3)"),
        }
    } else {
        CheckResult {
            name: "GCC toolchain (xtensa-esp-elf-gcc)",
            status: Status::Missing,
            version: None,
            note: Some(
                "Installed by espup — run: cargo binstall espup && espup install\n       \
                 Also ensure $HOME/.espup/... is on your PATH (source the export file)",
            ),
        }
    }
}

fn check_riscv_targets(env: &EnvState) -> CheckResult {
    let targets = [
        ("riscv32imc-unknown-none-elf",  "ESP32-C2, ESP32-C3"),
        ("riscv32imac-unknown-none-elf", "ESP32-C6, ESP32-H2"),
    ];

    let mut missing: Vec<String> = Vec::new();
    for (target, chips) in &targets {
        if !env.installed_riscv_targets.iter().any(|t| t == target) {
            missing.push(format!("{} ({})", target, chips));
        }
    }

    if missing.is_empty() {
        let names = targets.iter().map(|(t, _)| *t).collect::<Vec<_>>().join(", ");
        CheckResult {
            name: "RISC-V targets",
            status: Status::Ok,
            version: Some(names),
            note: Some("Covers ESP32-C2/C3 (riscv32imc) and ESP32-C6/H2 (riscv32imac)"),
        }
    } else {
        let fix = missing
            .iter()
            .map(|m| {
                let target = m.split_whitespace().next().unwrap_or("");
                format!("rustup target add {}", target)
            })
            .collect::<Vec<_>>()
            .join("\n       ");

        CheckResult {
            name: "RISC-V targets",
            status: Status::Warning(format!(
                "Missing targets:\n         {}\n       Fix:\n       {}",
                missing.join("\n         "),
                fix
            )),
            version: None,
            note: Some("Required for RISC-V based ESP chips"),
        }
    }
}

/// Run a command and return the first line of stdout, trimmed.
fn run_version_cmd(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .map(|s| s.lines().next().unwrap_or("").trim().to_string())
        })
        .filter(|s| !s.is_empty())
}
