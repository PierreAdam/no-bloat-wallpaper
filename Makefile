# Linux / macOS (and Windows with make installed). On Windows, prefer .\build.ps1
# Both programs are built into ./target/release (the GUI shares the CLI's target dir).
EXE := $(if $(filter Windows_NT,$(OS)),.exe,)

.PHONY: all cli gui run clean package

all: cli gui

cli:
	cargo build --release

gui:
	cargo build --release --manifest-path gui/src-tauri/Cargo.toml --target-dir target

package: cli
	cd gui/src-tauri && npx --yes @tauri-apps/cli@2.12.6 build

run: all
	./target/release/nbw-gui$(EXE)

clean:
	cargo clean
