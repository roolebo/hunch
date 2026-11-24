# Hunch for macOS

The tray of hunch on macOS. It runs [hunch-seat](../seat) as the
human's service, and hunch-host in place of any earlier build's, and
starts at login; its menu and settings, which
[TRAY.md](../seat/TRAY.md) describes, show what the service does
and needs, and set the seat.

    make
    open build/Hunch.app

On first launch it asks for the grants the service needs: Accessibility,
Microphone and Screen Recording. macOS ties them to
the app's signature, so with the default ad hoc signature it asks again
after each build; a certificate keeps them:

    make CERTIFICATE=NAME

They log to `~/Library/Logs/Hunch/hunch-seat.log` and `hunch-host.log`.
