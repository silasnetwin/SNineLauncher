// The launcher is a GUI app in both dev and release builds. Leaving debug builds
// as console applications makes Windows open an extra terminal when Tauri starts
// the native app (on top of the dev-server/build output in the invoking terminal).
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    snine_launcher_next_lib::run();
}
