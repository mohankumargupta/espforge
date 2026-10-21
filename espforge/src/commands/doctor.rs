// `espforge doctor`: verifies that prerequisites for building an ESPF project
// are met:
//
//   1. `cargo` is installed and runnable (needed to build generated projects).
//   2. Every RISC-V Rust target needed by the chips espforge supports is
//      installed. Chips and their target triples come from the
//      `esp-metadata-generated` crate, so the list follows the metadata
//      compiled into this binary instead of a hardcoded one.
//   3. (advisory) The `esp` toolchain from espup, needed only for the Xtensa
//      chips (esp32, esp32s2, esp32s3).
//   4. (advisory) `espflash` (default flasher) and `probe-rs` (only for
//      `--option probe-rs=true`).
//
// Required checks (1, 2) fail the command; advisory ones only warn.
//   exit 0 - every required check passed
//   exit 1 - at least one required check failed
use std::process::Command;

/// Chips `esp-metadata-generated` knows about that espforge has no template
/// or Wokwi board for yet. Remove an entry once it is supported.
const NOT_YET_SUPPORTED: [&str; 1] = ["esp32p4"];

enum Status {
    Pass(String),
    /// Advisory: reported, never fails the command.
    Warn(String),
    Fail(String),
}

pub fn run() {
    println!("espforge doctor");
    println!("===============\n");

    let checks: [(&str, fn() -> Status); 5] = [
        ("cargo", check_cargo),
        ("rustc targets", check_rustc_targets),
        ("xtensa toolchain", check_xtensa_toolchain),
        ("espflash", check_espflash),
        ("probe-rs", check_probe_rs),
    ];

    let (mut failed, mut warned) = (0, 0);
    for (name, check) in checks {
        match check() {
            Status::Pass(detail) => print_check_details('✔', name, &detail),
            Status::Warn(detail) => {
                warned += 1;
                print_check_details('!', name, &detail);
            }
            Status::Fail(problem) => {
                failed += 1;
                print_check_details('✖', name, &problem);
            }
        }
    }

    println!();
    if failed == 0 {
        if warned == 0 {
            println!("All prerequisites met.");
        } else {
            println!("Required prerequisites met ({warned} optional item(s) need attention).");
        }
    } else {
        println!("✖ {failed} check(s) failed; see the fix hints above.");
        std::process::exit(1);
    }
}

/// Prints `✔ name — <summary>` (first line of `details`) and indents any
/// further lines below the first, so a check's result can be a full report
/// instead of a single sentence.
fn print_check_details(mark: char, name: &str, details: &str) {
    let mut lines = details.lines();
    let first = lines.next().unwrap_or("");

    if first.is_empty() {
        println!("{mark} {name}");
    } else {
        println!("{mark} {name} — {first}");
    }
    for line in lines {
        if line.is_empty() {
            println!();
        } else {
            println!("      {line}");
        }
    }
}

/// Runs `cargo --version` and reports it.
fn check_cargo() -> Status {
    match version_output("cargo") {
        Ok(v) => Status::Pass(v),
        Err(_) => Status::Fail(
            "`cargo` was not found on PATH. Install Rust/cargo: https://rustup.rs".to_owned(),
        ),
    }
}

/// One chip paired with the Rust target triple a project for it is built for.
struct ChipTarget {
    name: &'static str,
    target: &'static str,
}

fn is_supported(chip: &esp_metadata_generated::Chip) -> bool {
    !NOT_YET_SUPPORTED.contains(&chip.name())
}

/// The RISC-V chips espforge supports, from the compiled-in esp-metadata.
/// Xtensa chips are built with the espup `esp` toolchain, not a rustup
/// target, so they are covered by [`check_xtensa_toolchain`] instead.
fn riscv_chip_targets() -> Vec<ChipTarget> {
    esp_metadata_generated::Chip::iter()
        .filter(|c| !c.is_xtensa() && is_supported(c))
        .map(|c| ChipTarget {
            name: c.name(),
            target: c.target(),
        })
        .collect()
}

fn xtensa_chips() -> Vec<&'static str> {
    esp_metadata_generated::Chip::iter()
        .filter(|c| c.is_xtensa() && is_supported(c))
        .map(|c| c.name())
        .collect()
}

/// Verifies that every RISC-V target the supported chips compile for is
/// installed, producing a per-target report instead of a bare pass/fail.
///
/// Prefers asking `rustup target list --installed`; if rustup is not on PATH
/// but `rustc` is (e.g. a distro toolchain), falls back to compiling an empty
/// `#![no_std]` probe per target and treating a `core` lookup failure as
/// "target not installed".
fn check_rustc_targets() -> Status {
    let chips = riscv_chip_targets();
    if chips.is_empty() {
        return Status::Fail(
            "expected esp-metadata to describe at least one RISC-V chip".to_owned(),
        );
    }

    // Collapse chips that share a target triple into groups, in chip order:
    // e.g. (esp32c2, esp32c3) both need riscv32imc-unknown-none-elf.
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for chip in &chips {
        match groups.iter_mut().find(|(target, _)| *target == chip.target) {
            Some((_, names)) => names.push(chip.name),
            None => groups.push((chip.target, vec![chip.name])),
        }
    }
    let targets: Vec<&str> = groups.iter().map(|(target, _)| *target).collect();

    let installed = match installed_targets(&targets) {
        Ok(i) => i,
        Err(e) => return Status::Fail(e),
    };

    let missing: Vec<&str> = targets
        .iter()
        .copied()
        .filter(|target| !installed.contains(target))
        .collect();

    // Human-readable report, one line per group:
    //   esp32c2, esp32c3       riscv32imc  ✔ installed
    let mut report = String::from("RISC-V Rust targets needed by the supported chips:");
    for (target, names) in &groups {
        let status = if installed.contains(target) {
            "✔ installed"
        } else {
            "✖ NOT INSTALLED"
        };
        report.push_str(&format!(
            "\n  {:<40} {}  {status}",
            names.join(", "),
            short_target(target)
        ));
    }

    if missing.is_empty() {
        report.push_str("\nall RISC-V targets installed.");
        Status::Pass(report)
    } else {
        let add_list = missing.join(" ");
        let short_missing = missing
            .iter()
            .map(|t| short_target(t))
            .collect::<Vec<_>>()
            .join(" ");
        Status::Fail(format!(
            "{report}\n\nmissing: {short_missing}. Fix with:\n  rustup target add {add_list}"
        ))
    }
}

/// Advisory: the Xtensa chips need the `esp` toolchain installed by espup.
fn check_xtensa_toolchain() -> Status {
    let chips = xtensa_chips();
    if chips.is_empty() {
        return Status::Pass("no Xtensa chips are supported by this build".to_owned());
    }
    let list = chips.join(", ");
    let hint = "To build for them:\n  cargo install espup --locked\n  espup install\n  source ~/export-esp.sh";

    if !which("rustup") {
        return Status::Warn(format!(
            "rustup not found, so the `esp` toolchain could not be checked (needed only for {list}).\n{hint}"
        ));
    }
    let output = Command::new("rustup").args(["toolchain", "list"]).output();
    match output {
        Ok(o) if o.status.success() => {
            if has_toolchain(&String::from_utf8_lossy(&o.stdout), "esp") {
                Status::Pass(format!("`esp` toolchain installed (for {list})"))
            } else {
                Status::Warn(format!(
                    "`esp` toolchain not installed; only needed for {list}.\n{hint}"
                ))
            }
        }
        _ => Status::Warn(format!(
            "could not run `rustup toolchain list` (needed only for {list}).\n{hint}"
        )),
    }
}

/// Is `name` listed by `rustup toolchain list`? Lines look like `esp` or
/// `stable-x86_64-unknown-linux-gnu (default)`; only an exact first token counts.
fn has_toolchain(list: &str, name: &str) -> bool {
    list.lines()
        .filter_map(|l| l.split_whitespace().next())
        .any(|t| t == name)
}

fn check_espflash() -> Status {
    check_tool(
        "espflash",
        "needed to flash with the default runner: cargo install espflash --locked",
    )
}

fn check_probe_rs() -> Status {
    check_tool(
        "probe-rs",
        "only needed for --option probe-rs=true: cargo install probe-rs-tools --locked",
    )
}

/// Advisory: an optional tool on PATH.
fn check_tool(binary: &str, why: &str) -> Status {
    match version_output(binary) {
        Ok(v) => Status::Pass(v),
        Err(_) => Status::Warn(format!("`{binary}` not found; {why}")),
    }
}

/// Returns the subset of `targets` that `rustc` can currently build for.
///
/// With `rustup` on PATH, asks `rustup target list --installed`; if rustup is
/// present but misbehaves (non-zero exit, unreadable output), that is a hard
/// error we cannot paper over. Without rustup (e.g. a distro toolchain), a
/// trivial `#![no_std]` probe per target decides usability instead.
fn installed_targets<'a>(targets: &[&'a str]) -> Result<Vec<&'a str>, String> {
    if which("rustup") {
        let output = Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output();
        let installed = output
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
            .ok_or_else(|| {
                "failed to read `rustup target list --installed`. Fix rustup: https://rustup.rs"
                    .to_owned()
            })?;

        return Ok(targets
            .iter()
            .copied()
            .filter(|target| installed.lines().any(|line| line.trim() == *target))
            .collect());
    }

    Ok(targets
        .iter()
        .copied()
        .filter(|target| target_compiles_no_std(target))
        .collect())
}

/// Drops the `-unknown-none-elf` suffix for terse display.
fn short_target(triple: &str) -> &str {
    triple.strip_suffix("-unknown-none-elf").unwrap_or(triple)
}

/// Returns true if a trivial `#![no_std]` lib compiles for `target`, i.e. at
/// least `core` is available. Returns false on any rustc invocation failure;
/// `installed_targets` wording still points the user at rustup.
fn target_compiles_no_std(target: &str) -> bool {
    let out_dir = std::env::temp_dir().join("espforge-doctor-probe");
    if !out_dir.exists() && std::fs::create_dir_all(&out_dir).is_err() {
        return false;
    }
    let source_path = out_dir.join("probe.rs");
    let output_path = out_dir.join("probe.rmeta");
    if std::fs::write(&source_path, "#![no_std]\n").is_err() {
        return false;
    }
    let output = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "--crate-type=lib",
            "--crate-name=espforge_probe",
            "--emit=metadata",
            "-o",
            output_path.to_string_lossy().as_ref(),
            "--target",
            target,
            source_path.to_string_lossy().as_ref(),
        ])
        .output();
    let _ = std::fs::remove_file(&source_path);
    let _ = std::fs::remove_file(&output_path);
    matches!(output, Ok(output) if output.status.success())
}

/// Returns true if a binary with this name can be located.
fn which(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(binary).is_file()))
}

/// Runs `binary --version` and returns its trimmed output line. Uses stderr as
/// a fallback in case the tool prints its version there.
fn version_output(binary: &str) -> Result<String, String> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|error| format!("failed to run `{binary} --version`: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    match (!stdout.is_empty(), !stderr.is_empty()) {
        (true, _) => Ok(stdout),
        (false, true) => Ok(stderr),
        (false, false) => Err(format!("`{binary} --version` produced no output")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riscv_targets_come_from_metadata_and_exclude_xtensa() {
        let chips = riscv_chip_targets();
        assert!(!chips.is_empty());
        assert!(chips.iter().any(|c| c.name == "esp32c3"));
        assert!(chips.iter().all(|c| c.target.starts_with("riscv32")));
        assert!(chips.iter().all(|c| !NOT_YET_SUPPORTED.contains(&c.name)));
    }

    #[test]
    fn xtensa_chips_are_listed_separately() {
        let x = xtensa_chips();
        assert!(x.contains(&"esp32s3"), "{x:?}");
        assert!(!x.contains(&"esp32c3"));
    }

    #[test]
    fn toolchain_list_parsing() {
        let list =
            "stable-x86_64-unknown-linux-gnu (default)\nesp\nnightly-x86_64-unknown-linux-gnu\n";
        assert!(has_toolchain(list, "esp"));
        assert!(!has_toolchain(
            "stable-x86_64-unknown-linux-gnu (default)\n",
            "esp"
        ));
        // a toolchain merely containing "esp" in its name does not count
        assert!(!has_toolchain("espressif-custom\n", "esp"));
    }

    #[test]
    fn short_target_drops_the_suffix() {
        assert_eq!(short_target("riscv32imc-unknown-none-elf"), "riscv32imc");
        assert_eq!(short_target("other"), "other");
    }
}
