use anyhow::Result;

/// `rgit version [--build-options]`
///
/// Prints `rgit version <version>` (the version in `Cargo.toml`). `--build-options` adds
/// what the binary was built for: CPU architecture, operating system, pointer size and
/// whether it is a debug or release build. Works outside a repository. Exit status: 0, or
/// 129 for an unknown option.
pub fn version(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit version [--build-options]\n\n    --[no-]build-options  also print build options\n\n";
    let mut build_options = false;
    for arg in &args {
        match arg.as_str() {
            "--build-options" => build_options = true,
            "--no-build-options" => build_options = false,
            "-h" => {
                print!("{}", USAGE);
                std::process::exit(129);
            }
            other if other.starts_with("--") => {
                eprint!("error: unknown option `{}'\n{}", &other[2..], USAGE);
                std::process::exit(129);
            }
            other if other.starts_with('-') && other.len() > 1 => {
                eprint!("error: unknown switch `{}'\n{}", other.chars().nth(1).unwrap_or('-'), USAGE);
                std::process::exit(129);
            }
            // Like git, plain arguments are ignored.
            _ => {}
        }
    }
    println!("rgit version {}", env!("CARGO_PKG_VERSION"));
    if build_options {
        println!("cpu: {}", std::env::consts::ARCH);
        println!("os: {}", std::env::consts::OS);
        println!("sizeof-size_t: {}", std::mem::size_of::<usize>());
        println!("profile: {}", if cfg!(debug_assertions) { "debug" } else { "release" });
    }
    Ok(())
}
