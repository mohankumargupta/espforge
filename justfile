# Default shell setup
set shell := ["bash", "-uc"]

# Default settings
default_out := env("ESPFORGE_OUT", "/tmp/espforge-demo")
default_example := "01.Basics/helloworld"
runtime_dir := justfile_directory() / "espforge-runtime"

# Default recipe: generates the default example and compiles it
default: (demo default_example default_out)

# Generate an example and build it in one step
demo example=default_example out=default_out *flags="": (gen example out flags) (build out)

# Alias for demo
run-example example=default_example out=default_out *flags="": (demo example out flags)

# Generate firmware project from an embedded example
gen example=default_example out=default_out *flags="":
    @echo "==> [espforge gen] Generating '{{ example }}' into '{{ out }}'..."
    cargo run -q -p espforge -- gen \
        --example "{{ example }}" \
        --out "{{ out }}" \
        --runtime-path "{{ runtime_dir }}" \
        {{ flags }}

# Build the generated project in the target directory using Cargo
build out=default_out *cargo_flags="":
    @echo "==> [cargo build] Compiling generated project in '{{ out }}'..."
    cd "{{ out }}" && cargo build {{ cargo_flags }}

# Clean the generated output directory
clean out=default_out:
    @echo "==> Cleaning '{{ out }}'..."
    rm -rf "{{ out }}"

# List all embedded examples available in the workspace
list-examples:
    @echo "Available embedded examples:"
    @find espforge-examples/src/examples -maxdepth 3 -name "example.espf.txt" 2>/dev/null \
        | sed -E 's|espforge-examples/src/examples/(.*)/example\.espf\.txt|  - \1|' \
        | sort
