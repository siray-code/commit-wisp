use std::{fs, path::PathBuf};

fn installer_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install.ps1");
    fs::read_to_string(path).expect("PowerShell installer should be readable")
}

fn release_workflow_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml");
    fs::read_to_string(path).expect("release workflow should be readable")
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

#[test]
fn release_publishes_bare_binaries_for_self_update() {
    let source = release_workflow_source();

    assert!(source
        .contains("commit-wisp-${{ matrix.target }}${{ runner.os == 'Windows' && '.exe' || '' }}"));
    assert!(source.contains("sha256sum commit-wisp-* install.sh install.ps1"));
}
