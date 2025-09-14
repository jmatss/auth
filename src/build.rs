use std::path::Path;

fn main() {
    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new().with_style("fluent".into()),
    )
    .unwrap();

    let android_api_level = 28;
    let ndk_path = std::env::var("ANDROID_NDK_ROOT").unwrap();
    let target_triple = std::env::var("TARGET").unwrap();
    let host_triple = std::env::var("HOST")
        .unwrap()
        .split('-')
        .map(|x| x.into())
        .collect::<Vec<String>>();

    let host_arch = &host_triple[0];
    let host_sys = &host_triple[2];

    let ndk_lib_path = Path::new(&ndk_path)
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(format!("{}-{}", host_sys, host_arch))
        .join("sysroot")
        .join("usr")
        .join("lib")
        .join(target_triple)
        .join(android_api_level.to_string());

    println!("cargo:rustc-link-search={}", ndk_lib_path.display());
    println!("cargo:rustc-link-lib=camera2ndk");
    println!("cargo:rustc-link-lib=mediandk");
}
