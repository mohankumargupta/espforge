export JUST_CHOOSER := "fzf --height=40% --tac --layout=reverse --border"

espforge_bin := justfile_directory() / "target/debug/espforge"
runtime_path := justfile_directory() / "espforge-runtime"
demo_dir     := "/tmp/espforgedemo"

_default:
  @just --list

# ---- build recipes -------------------------------------------------------

[group('espemu')]
helloworld: (_espforge "01.Basics/helloworld")

[group('espemu')]
blink: (_espforge "01.Basics/blink")

[group('espemu')]
i2cscanner: (_espforge "04.Communication/i2c_scanner")

[group('espemu')]
spiexample: (_espforge "04.Communication/spi_example")

[group('espemu')]
uartexample: (_espforge "04.Communication/uart_example")

[group('espemu')]
button: (_espforge "02.Digital/button")

# usage: (_emutest "<category>/<example>" "<pytest script>")

[group('espemu-test')]
helloworld_test: (_emutest "01.Basics/helloworld" "test_esphal.py")

[group('espemu-test')]
blink_test: (_emutest "01.Basics/blink" "test_blink.py")

# ---- shared recipes (the reusable "functions") ---------------------------

# Generate and compile one example into {{ demo_dir }}/build.
_espforge example dir=demo_dir:
  #!/usr/bin/env bash
  set -euxo pipefail
  rm -rf {{ dir }}
  mkdir {{ dir }}
  cargo build -p espforge
  {{ espforge_bin }} examples get {{ example }} --out {{ dir }}
  cd {{ dir }}
  {{ espforge_bin }} build example.espf.txt --runtime-path {{ runtime_path }}
  cd build
  cargo build

# Build an example, make a merged flash image, shim an ESP-IDF-style build dir
# for pytest-embedded, then run the given test script in esp-emu.
_emutest example script chip="esp32c3": (_espforge example)
  #!/usr/bin/env bash
  set -euo pipefail
  name="$(basename {{ example }})"
  build_dir="{{ demo_dir }}/build"
  shim=".esphal/$name/build"

  (cd "$build_dir" && cargo espflash save-image --merge firmware.factory.bin --chip {{ chip }})

  echo "Shimming an ESP-IDF-style build dir for pytest-embedded's IdfApp..."
  rm -rf ".esphal/$name"
  mkdir -p "$shim"
  cp -f "$build_dir/firmware.factory.bin" "$shim/firmware.factory.bin"
  cat > "$shim/flasher_args.json" <<EOF
  {
    "write_flash_args": ["--flash-mode", "dio"],
    "flash_files": {
      "0x0": "firmware.factory.bin"
    },
    "flash_settings": {
      "flash_mode": "dio"
    },
    "extra_esptool_args": {
      "chip": "{{ chip }}"
    }
  }
  EOF

  echo "Running {{ script }} against $name in esp-emu..."
  uv sync
  uv run pytest {{ script }} --embedded-services idf,espemu --app-path ".esphal/$name" -s

# ---- standalone esp-emu helpers ------------------------------------------

[no-cd]
esp-emu name assertion="Hello World" timeout="10s":
  #!/usr/bin/env bash
  firmware_location=".esphome/build/{{ name }}/build/firmware.factory.bin"
  echo "$firmware_location"
  set -x
  esp-emu --timeout {{ timeout }} --chip esp32c3 --firmware "$firmware_location" --exit-on "{{ assertion }}"

[no-cd]
esp-emu-uart name assertion="Hello World" timeout="20s":
  #!/usr/bin/env bash
  firmware_location=".esphome/build/{{ name }}/build/firmware.factory.bin"
  echo "$firmware_location"
  set -x
  esp-emu --timeout {{ timeout }} --chip esp32c3 --firmware "$firmware_location" --exit-on "{{ assertion }}" --uart1-tcp 127.0.0.1:5556
