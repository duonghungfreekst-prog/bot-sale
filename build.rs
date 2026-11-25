use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_windows_rc_exe(arch_dir: &str) -> Option<PathBuf> {
    if let Ok(path_var) = env::var("PATH") {
        for entry in env::split_paths(&path_var) {
            let candidate = entry.join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let kits_bin = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if let Ok(entries) = std::fs::read_dir(kits_bin) {
        let mut versions: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|v| v.path()))
            .filter(|p| p.is_dir())
            .collect();
        versions.sort();
        versions.reverse();
        for ver_dir in versions {
            let candidate = ver_dir.join(arch_dir).join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "msvc" {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let rc_input = manifest_dir.join("assets").join("app.rc");
    let ico_input = manifest_dir.join("assets").join("app.ico");
    if !rc_input.is_file() || !ico_input.is_file() {
        return;
    }

    let arch_dir = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x64",
        Ok("aarch64") => "arm64",
        _ => "x86",
    };

    let Some(rc_exe) = find_windows_rc_exe(arch_dir) else {
        println!("cargo:warning=Khong tim thay rc.exe; bo qua nhung icon vao .exe");
        return;
    };

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let res_output = out_dir.join("app.res");

    let status = Command::new(rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_output)
        .arg(&rc_input)
        .status();

    match status {
        Ok(s) if s.success() && res_output.is_file() => {
            println!("cargo:rustc-link-arg-bin=salehunter_bot={}", res_output.display());
        }
        Ok(s) => {
            println!("cargo:warning=rc.exe thoat voi ma {:?}; bo qua nhung icon.", s.code());
        }
        Err(err) => {
            println!("cargo:warning=Loi goi rc.exe ({}); bo qua nhung icon.", err);
        }
    }
}
