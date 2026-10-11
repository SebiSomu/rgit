use std::env;
use std::fs;
use std::process::Command;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut clean = false;
    let mut sub_dir = None;
    let mut rgit_args = Vec::new();
    let mut iter = env::args().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--clean" {
            clean = true;
        } else if arg == "--dir" {
            sub_dir = iter.next();
        } else {
            rgit_args.push(arg);
        }
    }

    let project_root = if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        PathBuf::from(manifest_dir)
    } else {
        let mut exe_path = env::current_exe()?;
        for _ in 0..3 {
            exe_path.pop();
        }
        exe_path
    };

    let sandbox_root = project_root.join("test-sandbox");
    let sandbox = match sub_dir {
        Some(s) => sandbox_root.join(s),
        None => sandbox_root.clone(),
    };


    if clean {
        if sandbox_root.exists() {
            println!("[rtest] Cleaning sandbox...");
            fs::remove_dir_all(&sandbox_root)?;
            println!("[rtest] Sandbox cleaned.");
        } else {
            println!("[rtest] Sandbox does not exist, nothing to clean.");
        }
        if rgit_args.is_empty() {
            return Ok(());
        }
    }

    println!("[rtest] Building rgit-main...");
    let build_status = Command::new("cargo")
        .arg("build")
        .arg("--bin")
        .arg("rgit-main")
        .current_dir(&project_root)
        .status()?;

    if !build_status.success() {
        eprintln!("[rtest] Build failed!");
        std::process::exit(1);
    }

    if !sandbox.exists() {
        fs::create_dir_all(&sandbox)?;
        println!("[rtest] Created sandbox: {}", sandbox.display());
    }
    #[cfg(target_os = "windows")]
    let bin_name = "rgit-main.exe";
    #[cfg(not(target_os = "windows"))]
    let bin_name = "rgit-main";

    let binary_path = project_root.join("target").join("debug").join(bin_name);

    println!("[rtest] Running: {} {}", bin_name, rgit_args.join(" "));
    println!("        (cwd: {})", sandbox.display());
    println!();

    let mut child = Command::new(binary_path)
        .args(&rgit_args)
        .current_dir(&sandbox)
        .spawn()?;

    let status = child.wait()?;
    std::process::exit(status.code().unwrap_or(0));
}
