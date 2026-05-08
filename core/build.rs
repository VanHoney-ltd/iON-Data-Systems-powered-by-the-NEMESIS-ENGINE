use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let profile = env::var("PROFILE").unwrap();

    let afcclient_src = PathBuf::from("src/afcclient");
    let c_sources = ["afcclient.c", "libidev.c"];
    let mut objects = Vec::new();

    // Only build afcclient on Linux and macOS
    if target_os != "linux" && target_os != "macos" {
        println!("cargo:warning=afcclient build skipped on unsupported OS: {}", target_os);
        return;
    }

    let compiler = "clang";

    // Compile each C source to an object in OUT_DIR
    for src in &c_sources {
        let src_path = afcclient_src.join(src);
        if !src_path.exists() {
            println!("cargo:warning=afcclient source missing: {}", src_path.display());
            return;
        }

        let obj_path = out_dir.join(src.replace(".c", ".o"));
        let mut cmd = Command::new(compiler);
        cmd.arg("-c")
            .arg("-o")
            .arg(&obj_path)
            .arg(&src_path);

        if target_os == "linux" {
            cmd.arg("-fblocks");
        }

        // Ensure POSIX declarations like struct stat are available
        cmd.arg("-D_DEFAULT_SOURCE");
        cmd.arg("-include").arg("sys/stat.h");

        // Include path for libidev.h
        cmd.arg(format!("-I{}", afcclient_src.display()));

        match cmd.output() {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    println!("cargo:warning=afcclient compile failed for {}: {}", src, stderr);
                    return;
                }
            }
            Err(e) => {
                println!("cargo:warning=Failed to run clang for {}: {}", src, e);
                return;
            }
        }

        objects.push(obj_path);
    }

    // Link the binary
    let bin_path = out_dir.join("afcclient");
    let mut cmd = Command::new(compiler);
    cmd.arg("-o").arg(&bin_path);
    for obj in &objects {
        cmd.arg(obj);
    }

    if target_os == "linux" {
        cmd.arg("-fblocks");
        cmd.arg("-lBlocksRuntime");
    }
    cmd.arg("-limobiledevice").arg("-lplist");

    match cmd.output() {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("cargo:warning=afcclient link failed: {}", stderr);
                return;
            }
        }
        Err(e) => {
            println!("cargo:warning=Failed to link afcclient: {}", e);
            return;
        }
    }

    // Also copy to target/{profile}/afcclient for convenience
    let target_dir = PathBuf::from("target").join(&profile);
    if let Ok(_) = std::fs::create_dir_all(&target_dir) {
        let _ = std::fs::copy(&bin_path, target_dir.join("afcclient"));
    }

    println!("cargo:rustc-env=AFCCLIENT_PATH={}", bin_path.display());
}
