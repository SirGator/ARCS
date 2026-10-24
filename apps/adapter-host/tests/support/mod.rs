use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// Cargo setzt CARGO_BIN_EXE nur für Binaries des eigenen Pakets. Die
/// Prozess-Tests bauen die separaten Adapter einmal je Testsuite und führen
/// anschließend deren echte Programme direkt aus, ohne Cargo als Transport.
pub fn adapter_binary(name: &str) -> PathBuf {
    static BINARIES: OnceLock<PathBuf> = OnceLock::new();
    let directory = BINARIES.get_or_init(|| {
        let executable = std::env::current_exe().expect("test executable path");
        let profile_directory = executable.parent().unwrap().parent().unwrap();
        let target_directory = profile_directory.parent().unwrap();
        let mut cargo = Command::new(env!("CARGO"));
        cargo
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "build",
                "--offline",
                "--locked",
                "--bins",
                "-p",
                "arcs-simulated-sensor",
                "-p",
                "arcs-simulated-fan",
                "-p",
                "arcs-file-data",
                "-p",
                "arcs-normalize-number",
                "-p",
                "arcs-file-output",
                "-p",
                "arcs-json-report",
                "-p",
                "arcs-json-input",
                "-p",
                "arcs-example-reasoner",
                "--target-dir",
            ])
            .arg(target_directory);
        match profile_directory.file_name().and_then(|name| name.to_str()) {
            Some("debug") => {}
            Some(profile) => {
                cargo.args(["--profile", profile]);
            }
            None => panic!("cannot determine Cargo profile"),
        }
        let build = cargo.output().expect("build external adapters");
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        profile_directory.to_path_buf()
    });
    directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}
