use std::{fs, path::PathBuf};

fn installer_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install.ps1");
    fs::read_to_string(path).expect("PowerShell installer should be readable")
}

#[test]
fn powershell_installer_falls_back_when_runtime_architecture_is_unavailable() {
    let source = installer_source();

    assert!(
        source.contains("$env:PROCESSOR_ARCHITECTURE"),
        "Windows PowerShell 5.1 may not expose RuntimeInformation.OSArchitecture"
    );
    assert!(
        source.contains("$env:PROCESSOR_ARCHITEW6432"),
        "32-bit PowerShell on 64-bit Windows needs PROCESSOR_ARCHITEW6432"
    );
}
