# Hunch for macOS

The tray of hunch on macOS. It runs [hunch-host](../host) as the
human's service and starts at login; its menu and settings, which
[docs/TRAY.md](../docs/TRAY.md) describes, show what the service does
and needs, and set the seat.

    make
    open build/Hunch.app

On first launch it asks for the grants the service needs: Accessibility,
Microphone and Screen Recording. macOS ties them to
the app's signature, so with the default ad hoc signature it asks again
after each build; a certificate keeps them:

    make CERTIFICATE=NAME

The service logs to `~/Library/Logs/Hunch/hunch-host.log`.
