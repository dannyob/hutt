PREFIX ?= ~/.local

.PHONY: build install install-macos-handler install-linux-handler \
        check test test-url-handler clippy clean

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

install-linux-handler:
	install -Dm755 macos/hutt-opener/Contents/MacOS/hutt-open.sh $(PREFIX)/bin/hutt-open
	install -Dm644 linux/hutt-opener.desktop ~/.local/share/applications/hutt-opener.desktop
	xdg-mime default hutt-opener.desktop x-scheme-handler/hutt
	@echo "Registered hutt:// URL scheme."

check:
	cargo check

test:
	cargo test

test-url-handler:
	bash tests/test-url-handler.sh

clippy:
	cargo clippy -- -W clippy::all

clean:
	cargo clean
