PREFIX ?= ~/.local

.PHONY: build install install-macos-handler check test clippy clean

build:
	cargo build --release

install: build
	cargo install --path . --root $(PREFIX)

install-macos-handler:
	@echo "Building Hutt Opener.app with osacompile..."
	\rm -rf ~/Applications/"Hutt Opener.app"
	osacompile -o ~/Applications/"Hutt Opener.app" macos/hutt-opener.applescript
	\cp macos/hutt-opener/Contents/Info.plist ~/Applications/"Hutt Opener.app"/Contents/Info.plist
	\cp macos/hutt-opener/Contents/MacOS/hutt-open.sh ~/Applications/"Hutt Opener.app"/Contents/MacOS/hutt-open.sh
	chmod +x ~/Applications/"Hutt Opener.app"/Contents/MacOS/hutt-open.sh
	/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f ~/Applications/"Hutt Opener.app"
	@echo "Registered hutt:// URL scheme."

check:
	cargo check

test:
	cargo test

clippy:
	cargo clippy -- -W clippy::all

clean:
	cargo clean
