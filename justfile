set shell := ["sh", "-c"]
set windows-shell := ["powershell", "-c"]

_main:
    @just --list

prerequisites:
    cargo install cargo-binstall
    cargo binstall espup
    cargo binstall esp-generate

update:
    espup update
    cargo binstall esp-generate

build:
    cargo build

test:
    cargo build
    cargo test

check:
    cargo check

run:
    cargo run

minimal:
    cargo run -- compile examples/001-minimal.yaml

examples:
    cargo run -- examples

test_with_output:
    cargo test -- --no-capture

format:
    cargo fmt

lint:
    cargo clippy

tidy:
    @just format
    @just lint

fix:
    cargo clippy --fix --bin espforge

espgenerate:
    esp-generate --chip esp32c3 -o esp-backtrace -o vscode blank



