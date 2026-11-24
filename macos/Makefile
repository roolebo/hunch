# make app builds build/Hunch.app: the tray, with hunch-seat, hunch-host,
# hunch-9p and hunch-browser-native inside. CERTIFICATE signs it; with the
# default, ad hoc, macOS asks for its grants again after each build.

CERTIFICATE ?= -
APP = build/Hunch.app
SEAT = ../seat/target/release/hunch-seat
HOST = ../host/target/release

.PHONY: app programs
app: programs
	rm -rf $(APP)
	mkdir -p $(APP)/Contents/MacOS
	cp Info.plist $(APP)/Contents/
	mkdir -p $(APP)/Contents/Resources
	swiftc -O -o build/icon icon.swift
	build/icon build/AppIcon.iconset
	iconutil -c icns -o $(APP)/Contents/Resources/AppIcon.icns \
		build/AppIcon.iconset
	cp $(SEAT) $(HOST)/hunch-host $(HOST)/hunch-9p \
		$(HOST)/hunch-browser-native $(APP)/Contents/MacOS/
	swiftc -O -target arm64-apple-macos13 -o $(APP)/Contents/MacOS/Hunch Tray.swift
	codesign -f -s '$(CERTIFICATE)' -i systems.subtile.hunch.seat \
		$(APP)/Contents/MacOS/hunch-seat
	codesign -f -s '$(CERTIFICATE)' \
		$(APP)/Contents/MacOS/hunch-host $(APP)/Contents/MacOS/hunch-9p \
		$(APP)/Contents/MacOS/hunch-browser-native
	codesign -f -s '$(CERTIFICATE)' $(APP)
	@echo built $(APP)

programs:
	cargo build --release --manifest-path ../seat/Cargo.toml
	cargo build --release --manifest-path ../host/Cargo.toml
