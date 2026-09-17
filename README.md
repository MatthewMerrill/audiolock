# AudioLock

When you change the Default Output Device,

Then changes the Default Communications Output Device.

## Build on DevContainer:

```bash
cargo xwin build --target x86_64-pc-windows-msvc --release
```

Download to `%AppData%\Microsoft\Windows\Start Menu\Programs\Startup`
to launch in the system tray whenever you log in.
