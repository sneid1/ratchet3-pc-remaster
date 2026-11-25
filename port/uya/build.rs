use std::{env, path::PathBuf, process::Command};

fn main() {
    // The Streamline layer (native/streamline.cpp) is built with the SDK's headers: STREAMLINE_SDK is the SDK's folder (the one with
    // include/sl.h), set like DLSS_SDK.
    println!("cargo:rerun-if-changed=native/streamline.cpp");
    println!("cargo:rerun-if-env-changed=STREAMLINE_SDK");
    let sdk = env::var("STREAMLINE_SDK").expect("STREAMLINE_SDK is not set (the NVIDIA Streamline SDK folder, the one with include/sl.h; docs/WORKFLOW.md, \"DLSS\")");
    cc::Build::new().cpp(true).std("c++17").file("native/streamline.cpp").include(PathBuf::from(sdk).join("include")).compile("uya_streamline");
    println!("cargo:rerun-if-changed=assets/portrait.ico");
    println!("cargo:rerun-if-changed=assets/portrait.rc");
    if env::var("TARGET").unwrap().ends_with("windows-msvc") {
        let resource = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("portrait.res");
        let status = Command::new("rc.exe").current_dir("assets")
            .arg("/nologo").arg("/fo").arg(&resource).arg("portrait.rc")
            .status().expect("compile the game icon with the Windows SDK resource compiler");
        assert!(status.success(), "icon resource compilation failed");
        println!("cargo:rustc-link-arg-bin=uya={}", resource.display());
    }
}
