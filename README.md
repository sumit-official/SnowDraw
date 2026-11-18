# SnowDraw 0.2.0 — XFCE/X11 prototype

SnowDraw is a Rust desktop annotation prototype designed first for **XFCE running on X11**.
This release deliberately does not target Niri, Hyprland, or Wayland.

## What this prototype includes

- A separate, transparent, full-screen annotation surface.
- **Interact mode** by default: an empty X11 input shape makes pointer events pass through to windows underneath.
- **Draw mode**: the overlay receives pointer events and draws red pen strokes.
- A separate floating toolbar that can be dragged by its `SnowDraw ⋮⋮` grip.
- A confirmation dialog before **Erase all** clears every annotation.
- **Undo / Redo** for desktop strokes, and a **pencil size** control (five preset sizes, thin to huge).
- **Mouse / Pencil / Eraser** tool selector (like Epic Pen): *Mouse* clicks straight through to your apps, *Pencil* draws, and the eraser removes any stroke you click or drag over (a ring shows its reach). **Erase all** still clears everything.
- **Color**: six quick swatches plus a full color picker (Color…).
- The toolbar is never drawn on: in Draw mode the overlay leaves a hole where the toolbar is, so you can always click tools or Exit with the mouse.
- A visible **Exit ×** button on the toolbar that destroys the overlay and toolbar before quitting.

## Current limitations

This is a first XFCE/X11 behavior prototype, not a feature-complete Epic Pen replacement.

- GTK 3 and the X11 Shape extension are required.
- The overlay covers the primary X screen geometry; multi-monitor layouts need further testing and work.
- Drawing is currently a basic pen plus stroke eraser. Highlighter, shapes, arrows, text-on-desktop, pressure, and redo are not implemented yet.
- Keyboard shortcuts and autostart integration are not implemented.
- The project compiles and launches on an X11 system, but its interactive behavior still needs testing on an XFCE/X11 desktop before relying on it.

## Dependencies (Debian/Ubuntu examples)

```bash
sudo apt install build-essential pkg-config libgtk-3-dev libcairo2-dev
```

On Arch Linux:

```bash
sudo pacman -S --needed base-devel pkgconf gtk3 cairo
```

Install Rust with rustup if `cargo` is not already available.

## Build and run

Make sure the XFCE session is using X11. Open a terminal in the extracted
project directory (the directory containing `Cargo.toml`), then run:

```bash
cargo build --release
GDK_BACKEND=x11 ./target/release/snowdraw
```

If your terminal prompt already ends with `snowdraw-0.2.0`, you are already
inside the project directory: **do not run `cd snowdraw-0.2.0` again**.

The program starts in **Interact** mode. Click **Draw** to capture the mouse for annotation; click **Interact** to return to normal desktop interaction. Drag the toolbar by its grip. Click **Exit ×** to close all SnowDraw windows and quit the application.

## Troubleshooting

- If the overlay is opaque, check that the X server supports compositing and that XFCE compositing is enabled.
- If clicks do not pass through in Interact mode, verify the X11 Shape extension is available (`xdpyinfo | grep -i shape`) and that the app is running with `GDK_BACKEND=x11`.
- If the session is Wayland, this X11 prototype is not the correct backend.

## License

MIT


## GTK 0.18 compatibility patch

This source revision addresses the GTK-rs 0.18 compiler diagnostics reported by a tester:

- Event callbacks return `glib::Propagation::Proceed` instead of the removed `Inhibit` wrapper.
- Monitor selection uses `GdkDisplay::primary_monitor()` (falling back to monitor 0) and `GdkMonitor::geometry()`.

Build with `cargo build --release` on an XFCE/X11 system with the GTK 3 development libraries installed. This revision was compiled and launched on Arch Linux x86_64; full desktop interaction testing remains outstanding.
