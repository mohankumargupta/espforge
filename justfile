set shell := ["sh", "-c"]
set windows-shell := ["powershell", "-c"]

_main:
    @just --list
    
prerequisites:
  cargo install cargo-binstall
  cargo install espup
  espup update
  cargo binstall esp-generate
  cargo binstall espforge
  
prerequisites_publish:
  cargo binstall release-plz
  cargo binstall cargo-semver-checks

build:
  cargo build -p esforge
  
# when editing espforge_examples need to do this
clean:
  cargo clean
 
tidy:
    @just format
    @just lint
  
copy_generated name rust_project_path espforge_path:
  #!powershell.exe
  $destRoot = "{{espforge_path}}/espforge_examples_generated/{{name}}"
  $sourceRoot = "{{rust_project_path}}"
  
  # Clean destination
  if (Test-Path $destRoot) {
    Remove-Item -Path $destRoot -Recurse -Force
  }
  New-Item -ItemType Directory -Path $destRoot -Force | Out-Null
  
  # Files to copy (use Copy-Item)
  $files = @('build.rs', 'rust-toolchain.toml', 'wokwi.toml', 'Cargo.toml', 'diagram.json', 'config.toml')
  foreach ($file in $files) {
    $sourceFile = Join-Path $sourceRoot $file
    if (Test-Path $sourceFile) {
      Copy-Item -Path $sourceFile -Destination $destRoot -Force
    }
  }
  
  # Directories to copy (use robocopy)
  $dirs = @('.cargo', 'src')
  foreach ($dir in $dirs) {
    $sourceDir = Join-Path $sourceRoot $dir
    $destDir = Join-Path $destRoot $dir
    if (Test-Path $sourceDir) {
      robocopy $sourceDir $destDir /E /NFL /NDL /NJH /NJS /nc /ns /np 2>$null
    }
  }

# dry_run_std_crates:


# dry_run_esp32_crates:

# publish_std_crates:
  

# publish_esp32_crates:

# publish: dry_run = True, level=patch
#   cargo publish --workspace level dry_run
    
# test_with_output:
#   cargo test -- --no-capture
