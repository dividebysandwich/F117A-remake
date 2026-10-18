//! Linux: the executable's RUNPATH is its own directory, so `dlopen("libopenxr_loader.so")`
//! (bevy_mod_openxr, `xr::entry`) finds the OpenXR loader the release packages ship beside it
//! (SteamOS and the Steam Frame have none) before looking in the system directories.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
}
