use std::path::{Path, PathBuf};

fn main() {
    let is_release_mode = std::env::var("PROFILE").is_ok_and(|a| a == "release");

    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new().with_style("fluent".into()),
    )
    .unwrap();

    let android_jar_path = android_build::android_jar(None).unwrap();
    let out_dir_path = PathBuf::from(&std::env::var("OUT_DIR").unwrap());
    let java_file_path = Path::new("src").join("java").join("CameraHelper.java");

    let compile_exit_status = android_build::JavaBuild::new()
        .class_path(&android_jar_path)
        .classes_out_dir(&out_dir_path)
        .file(&java_file_path)
        .compile()
        .unwrap();

    if !compile_exit_status.success() {
        panic!("Java compile failed");
    }

    let dexer_exit_status = android_build::Dexer::new()
        .android_jar(&android_jar_path)
        .class_path(&out_dir_path)
        .out_dir(&out_dir_path)
        .release(is_release_mode)
        .android_min_api(20)
        .collect_classes(&out_dir_path)
        .unwrap()
        .command()
        .unwrap()
        .output()
        .unwrap()
        .status;

    if !dexer_exit_status.success() {
        panic!("Dexer failed");
    }

    println!("cargo:rerun-if-changed={}", java_file_path.display());
}
