//! Linux: the executable's RUNPATH is its own directory, so `dlopen("libopenxr_loader.so")`
//! (bevy_mod_openxr, `xr::entry`) finds the OpenXR loader the release packages ship beside it
//! (SteamOS and the Steam Frame have none) before looking in the system directories.
//!
//! Windows: links in the executable's icon, manifest and version (packaging/windows).

use std::path::Path;

fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "linux" {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
    // The icon in stealth-mission-89.exe, for Explorer, shortcuts and the taskbar.
    if os == "windows" {
        println!("cargo::rerun-if-changed=packaging/windows/stealth-mission-89.rc");
        println!("cargo::rerun-if-changed=packaging/windows/stealth-mission-89.ico");
        println!("cargo::rerun-if-changed=packaging/windows/stealth-mission-89.manifest");
        let out_dir = std::env::var("OUT_DIR").unwrap();
        version_resource(Path::new(&out_dir));
        embed_resource::compile_for(
            "packaging/windows/stealth-mission-89.rc",
            ["stealth-mission-89"],
            embed_resource::ParamsIncludeDirs([&out_dir]),
        )
        .manifest_required()
        .unwrap();
    }
}

/// Writes version.rc into `out_dir` for packaging/windows/stealth-mission-89.rc to include: the
/// version and names that Explorer shows on the Details tab of the executable's properties.
fn version_resource(out_dir: &Path) {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    let numbers = format!(
        "{},{},{},0",
        var("CARGO_PKG_VERSION_MAJOR"),
        var("CARGO_PKG_VERSION_MINOR"),
        var("CARGO_PKG_VERSION_PATCH")
    );
    let version = var("CARGO_PKG_VERSION");
    let strings = [
        ("CompanyName", "dividebysandwich"),
        ("FileDescription", "Stealth Mission 89"),
        ("FileVersion", &version),
        ("InternalName", "stealth-mission-89"),
        ("LegalCopyright", "Apache-2.0"),
        ("OriginalFilename", "stealth-mission-89.exe"),
        ("ProductName", "Stealth Mission 89"),
        ("ProductVersion", &version),
    ];
    // An application (FILETYPE 1) for 32-bit Windows (FILEOS 0x40004).
    let mut rc = format!(
        "1 VERSIONINFO\nFILEVERSION {numbers}\nPRODUCTVERSION {numbers}\nFILEFLAGSMASK 0x3F\nFILEFLAGS 0\nFILEOS 0x40004\nFILETYPE 1\nFILESUBTYPE 0\nBEGIN\n  BLOCK \"StringFileInfo\"\n  BEGIN\n    BLOCK \"040904B0\"\n    BEGIN\n"
    );
    for (key, value) in strings {
        rc += &format!("      VALUE \"{key}\", \"{value}\"\n");
    }
    // US English, Unicode.
    rc += "    END\n  END\n  BLOCK \"VarFileInfo\"\n  BEGIN\n    VALUE \"Translation\", 0x409, 1200\n  END\nEND\n";
    std::fs::write(out_dir.join("version.rc"), rc).unwrap();
}
