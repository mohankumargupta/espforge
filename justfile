export JUST_CHOOSER := "fzf --height=40% --tac --layout=reverse --border"

_default:
  @just --list

[group('espemu')]
helloworld:
  echo "helloworld"

[group('espemu')]
blink:
  #!/usr/bin/env bash
  set -e 
  just _espforge blink

_espforge example="helloworld" dir="/tmp/espforgedemo" :
  #!/usr/bin/env bash
  set -x
  rm -rf {{ dir }}
  mkdir {{ dir }}
  cargo build
  runtime_path="/home/pi/junk/espforge-experiments/espf-parsing-elegant/espforge-runtime"
  espforge examples get 01.Basics/{{ example }} --out {{ dir }}
  cd {{ dir }}
  espforge build example.espf.txt --runtime-path $runtime_path 
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


#   uvx --with=pip esphome version
#
# compile yaml_file:
#   uvx --with=pip esphome compile {{ yaml_file}}
#
# upload yaml_file:
#   # run this with esp-emu --chip esp32c3 --uart socket://127.0.0.1:5555
#   uvx --with=pip esphome upload {{ yaml_file }} --device serial://socket://127.0.0.1:5555
#
# merge_bin:
#   #!/usr/bin/env bash
#   pwd
#   cargo espflash save-image --merge firmware.factory.bin --chip esp32c3
#
# test:
#   uv run pytest test.py --embedded-services idf,espemu --app-path .esphome/build/helloworldc3 --espemu-image-path "{{ justfile_directory() }}/.esphome/build/helloworldc3/build/firmware.factory.bin" -s
#
# Runs test_esphal.py against the esp-hal (Rust) "helloworld" project in esp-emu.
# pytest-embedded expects an ESP-IDF build dir (flasher_args.json etc.), which a
# cargo project doesn't have, so we build the merged factory image with
# cargo-espflash and shim an IDF-style build dir under .esphal/ for IdfApp to parse.

#   #!/usr/bin/env bash
#   set -e
#   build_dir="{{ out }}"
#   mkdir -p "${build_dir}"
#   echo "Building the esp-hal (Rust) firmware image..."
#   (cd "$build_dir" && cargo build && cargo espflash save-image --merge firmware.factory.bin --chip esp32c3)
#   echo "Shimming an ESP-IDF-style build dir for pytest-embedded's IdfApp..."
#   shim=".esphal/helloworld/build"
#   mkdir -p "$shim"
#   cp -f "$build_dir/firmware.factory.bin" "$shim/firmware.factory.bin"
#   cat > "$shim/flasher_args.json" <<'EOF'
#   {
#     "write_flash_args": ["--flash-mode", "dio"],
#     "flash_files": {
#       "0x0": "firmware.factory.bin"
#     },
#     "flash_settings": {
#       "flash_mode": "dio"
#     },
#     "extra_esptool_args": {
#       "chip": "esp32c3"
#     }
#   }
#   EOF
#   echo "Running test_esphal.py against the esp-hal firmware in esp-emu..."
#   uv run pytest test_esphal.py --embedded-services idf,espemu --app-path .esphal/helloworld -s
#


[no-cd]
esp-emu name assertion="Hello World" timeout="10s":
  #!/usr/bin/env bash
  firmware_location=".esphome/build/{{ name }}/build/firmware.factory.bin"
  echo "$firmware_location"
  #esp-emu --chip esp32c3 --strap-mode 0x02 --uart-tcp 127.0.0.1:5555
  set -x
  esp-emu --timeout 10s --chip esp32c3 --firmware "$firmware_location" --exit-on "{{ assertion}}i" 

[no-cd]
esp-emu-uart name assertion="Hello World" timeout="20s"  :
  #!/usr/bin/env bash
  firmware_location=".esphome/build/{{ name }}/build/firmware.factory.bin"
  echo "$firmware_location"
  #esp-emu --chip esp32c3 --strap-mode 0x02 --uart-tcp 127.0.0.1:5555
  set -x
  esp-emu --timeout 10s --chip esp32c3 --firmware "$firmware_location" --exit-on "{{ assertion}}" --uart1-tcp 127.0.0.1:5556  

# clean:
#   uvx --with=pip esphome clean-all
#
#
#
