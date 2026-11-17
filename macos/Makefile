# CERTIFICATE signs the app; with the default, ad hoc, macOS asks for its
# grants again after each build.

CERTIFICATE ?= -
APP = build/Hunch.app
HOST = ../host/target/release/hunch-host

.PHONY: app hunch-host
app: hunch-host
	rm -rf $(APP)
	mkdir -p $(APP)/Contents/MacOS
	cp Info.plist $(APP)/Contents/
	mkdir -p $(APP)/Contents/Resources
	swiftc -O -o build/icon icon.swift
	build/icon build/AppIcon.iconset
	iconutil -c icns -o $(APP)/Contents/Resources/AppIcon.icns build/AppIcon.iconset
	cp $(HOST) $(APP)/Contents/MacOS/
	swiftc -O -target arm64-apple-macos13 -o $(APP)/Contents/MacOS/Hunch Tray.swift
	codesign -f -s '$(CERTIFICATE)' -i systems.subtile.hunch.host $(APP)/Contents/MacOS/hunch-host
	codesign -f -s '$(CERTIFICATE)' $(APP)
	@echo built $(APP)

hunch-host:
	cargo build --release --manifest-path ../host/Cargo.toml
