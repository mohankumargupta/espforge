export JUST_CHOOSER := "fzf --height=40% --tac --layout=reverse --border"

espforge_bin := justfile_directory() / "target/debug/espforge"
runtime_path := justfile_directory() / "espforge-runtime"

_default:
  @just --list

[group('espemu')]
helloworld:
  just _espforge helloworld

[group('espemu')]
blink:
  just _espforge blink

_espforge example="helloworld" dir="/tmp/espforgedemo" :
  #!/usr/bin/env bash
  set -ex
  rm -rf {{ dir }}
  mkdir {{ dir }}
  cargo build -p espforge
  {{ espforge_bin }} examples get 01.Basics/{{ example }} --out {{ dir }}
  cd {{ dir }}
  {{ espforge_bin }} build example.espf.txt --runtime-path {{ runtime_path }}
  cd build
  cargo build

espforgetest out="/tmp/espforgedemo/build":
  #!/usr/bin/env bash
  set -e
  build_dir="{{ out }}"
  (cd "$build_dir" && cargo espflash save-image --merge firmware.factory.bin --chip esp32c3)
  echo "Shimming an ESP-IDF-style build dir for pytest-embedded's IdfApp..."
  shim=".esphal/helloworld/build"
  mkdir -p "$shim"
  cp -f "$build_dir/firmware.factory.bin" "$shim/firmware.factory.bin"
  cat > "$shim/flasher_args.json" <<'EOF'
  {
    "write_flash_args": ["--flash-mode", "dio"],
    "flash_files": {
      "0x0": "firmware.factory.bin"
    },
    "flash_settings": {
      "flash_mode": "dio"
    },
    "extra_esptool_args": {
      "chip": "esp32c3"
    }
  }
  EOF
  echo "Running test_esphal.py against the esp-hal firmware in esp-emu..."
  uv sync
  uv run pytest test_esphal.py --embedded-services idf,espemu --app-path .esphal/helloworld -s

[no-cd]
esp-emu name assertion="Hello World" timeout="10s":
  #!/usr/bin/env bash
  firmware_location=".esphome/build/{{ name }}/build/firmware.factory.bin"
  echo "$firmware_location"
  set -x
  esp-emu --timeout 10s --chip esp32c3 --firmware "$firmware_location" --exit-on "{{ assertion}}i"

[no-cd]
esp-emu-uart name assertion="Hello World" timeout="20s"  :
  #!/usr/bin/env bash
  firmware_location=".esphome/build/{{ name }}/build/firmware.factory.bin"
  echo "$firmware_location"
  set -x
  esp-emu --timeout 10s --chip esp32c3 --firmware "$firmware_location" --exit-on "{{ assertion}}" --uart1-tcp 127.0.0.1:5556
