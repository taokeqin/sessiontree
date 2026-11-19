use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let plist = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("Info.plist");
    fs::write(
        &plist,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.hacken.sessiontree</string>
<key>CFBundleName</key><string>Sessiontree</string>
<key>CFBundleDisplayName</key><string>Sessiontree</string>
<key>CFBundleExecutable</key><string>sessiontree</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#,
    )
    .expect("write embedded macOS application identity");
    println!(
        "cargo:rustc-link-arg-bin=sessiontree=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        plist.display()
    );
}
