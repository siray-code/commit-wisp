//! Self-update support.

use std::{
    env,
    io::Write,
    path::{Component, Path},
    process::Command,
    time::Duration,
};

use anyhow::{Context, Result};
use futures_util::StreamExt;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/siray-code/commit-wisp/releases/latest";
const MAX_BINARY_BYTES: usize = 100 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

/// Updates the current executable or delegates to Homebrew when it owns it.
pub async fn update() -> Result<()> {
    let current_exe = env::current_exe().context("Could not locate current executable")?;
    let resolved_exe = current_exe
        .canonicalize()
        .unwrap_or_else(|_| current_exe.clone());
    if is_homebrew_path(&resolved_exe) {
        return update_with_homebrew();
    }

    let client = reqwest::Client::builder()
        .user_agent(concat!("commit-wisp/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(60))
        .build()
        .context("Could not initialize update client")?;
    let release = fetch_release(&client, LATEST_RELEASE_URL).await?;
    let latest = validate_release(&release)?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .context("Installed commit-wisp version is invalid")?;
    if !update_available(&current, &latest) {
        println!("Already up to date: {current}");
        return Ok(());
    }

    let target = target_triple(env::consts::OS, env::consts::ARCH)?;
    let asset_name = asset_name(target, env::consts::OS);
    let asset_url = release_asset_url(&release, &asset_name)?;
    let checksums_url = release_asset_url(&release, "SHA256SUMS")?;
    let new_binary = download_verified(
        &client,
        asset_url,
        checksums_url,
        &asset_name,
        current_exe
            .parent()
            .context("Executable has no parent directory")?,
    )
    .await?;

    let new_binary = new_binary.into_temp_path();
    self_replace::self_replace(&new_binary)
        .context("Could not replace current executable; the existing version was preserved")?;
    println!("Updated commit-wisp: {current} -> {latest}");
    Ok(())
}

fn update_with_homebrew() -> Result<()> {
    let status = Command::new("brew")
        .args(["upgrade", "commit-wisp"])
        .status()
        .context("Homebrew installation detected, but brew could not be executed")?;
    anyhow::ensure!(status.success(), "Homebrew upgrade failed");
    println!("Homebrew upgrade completed");
    Ok(())
}

async fn fetch_release(client: &reqwest::Client, url: &str) -> Result<Release> {
    let bytes = download(client, url, MAX_METADATA_BYTES)
        .await
        .context("Could not fetch latest release")?;
    serde_json::from_slice(&bytes).context("GitHub returned invalid release metadata")
}

fn validate_release(release: &Release) -> Result<Version> {
    anyhow::ensure!(
        !release.draft && !release.prerelease,
        "Latest GitHub release is not stable"
    );
    parse_release_version(&release.tag_name)
}

fn parse_release_version(tag: &str) -> Result<Version> {
    let version = tag
        .strip_prefix('v')
        .context("Release tag must start with v")?;
    let version = Version::parse(version).context("Release tag is not semantic version")?;
    anyhow::ensure!(
        version.pre.is_empty(),
        "Prerelease versions are not eligible for automatic updates"
    );
    Ok(version)
}

fn update_available(current: &Version, latest: &Version) -> bool {
    latest > current
}

fn release_asset_url<'a>(release: &'a Release, name: &str) -> Result<&'a str> {
    let url = release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .map(|asset| asset.browser_download_url.as_str())
        .with_context(|| format!("Release does not contain {name}"))?;
    anyhow::ensure!(
        url.starts_with("https://github.com/siray-code/commit-wisp/releases/download/"),
        "Release asset URL is outside the official repository"
    );
    Ok(url)
}

async fn download_verified(
    client: &reqwest::Client,
    asset_url: &str,
    checksums_url: &str,
    asset_name: &str,
    destination_dir: &Path,
) -> Result<NamedTempFile> {
    let checksums = download(client, checksums_url, MAX_METADATA_BYTES)
        .await
        .context("Could not download SHA256SUMS")?;
    let checksums = std::str::from_utf8(&checksums).context("SHA256SUMS is not UTF-8")?;
    let expected = checksum_for(checksums, asset_name)?;
    let binary = download(client, asset_url, MAX_BINARY_BYTES)
        .await
        .with_context(|| format!("Could not download {asset_name}"))?;
    let actual = format!("{:x}", Sha256::digest(&binary));
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(&expected),
        "Checksum verification failed for {asset_name}"
    );

    let mut file = NamedTempFile::new_in(destination_dir)
        .context("Could not create update file beside current executable")?;
    file.write_all(&binary)
        .context("Could not write update file")?;
    file.flush().context("Could not flush update file")?;
    file.as_file()
        .sync_all()
        .context("Could not sync update file")?;
    set_executable(file.path())?;
    Ok(file)
}

async fn download(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .context("Download request failed")?
        .error_for_status()
        .context("Download returned an error status")?;
    if let Some(length) = response.content_length() {
        anyhow::ensure!(length <= limit as u64, "Download exceeds size limit");
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Download was interrupted")?;
        anyhow::ensure!(
            bytes.len() + chunk.len() <= limit,
            "Download exceeds size limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn checksum_for(contents: &str, asset_name: &str) -> Result<String> {
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let Some(checksum) = fields.next() else {
            continue;
        };
        let Some(name) = fields.next() else { continue };
        if name.trim_start_matches('*') == asset_name {
            anyhow::ensure!(
                checksum.len() == 64 && checksum.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "Invalid checksum for {asset_name}"
            );
            return Ok(checksum.to_ascii_lowercase());
        }
    }
    anyhow::bail!("{asset_name} is missing from SHA256SUMS")
}

fn target_triple(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        _ => anyhow::bail!("Unsupported update platform: {os}/{arch}"),
    }
}

fn asset_name(target: &str, os: &str) -> String {
    if os == "windows" {
        format!("commit-wisp-{target}.exe")
    } else {
        format!("commit-wisp-{target}")
    }
}

fn is_homebrew_path(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    components
        .windows(2)
        .any(|pair| pair == ["Cellar", "commit-wisp"])
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .context("Could not mark update executable")
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read, Write},
        net::TcpListener,
        path::Path,
        thread,
    };

    use semver::Version;
    use sha2::{Digest, Sha256};

    use super::{
        asset_name, checksum_for, download_verified, is_homebrew_path, parse_release_version,
        target_triple, update_available,
    };

    fn update_server(
        asset_name: &str,
        binary: &[u8],
        checksum: &str,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind update server");
        let address = listener.local_addr().expect("update server address");
        let asset_name = asset_name.to_owned();
        let binary = binary.to_vec();
        let checksum_body = format!("{checksum}  {asset_name}\n").into_bytes();
        let handle = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept update request");
                let mut request = [0_u8; 4096];
                let count = stream.read(&mut request).expect("read update request");
                let request = String::from_utf8_lossy(&request[..count]);
                let body = if request.starts_with("GET /SHA256SUMS ") {
                    &checksum_body
                } else if request.starts_with(&format!("GET /{asset_name} ")) {
                    &binary
                } else {
                    panic!("unexpected update request: {request}");
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .expect("write response headers");
                stream.write_all(body).expect("write response body");
            }
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn maps_supported_release_targets() {
        assert_eq!(
            target_triple("macos", "aarch64").expect("Apple Silicon"),
            "aarch64-apple-darwin"
        );
        assert_eq!(
            target_triple("macos", "x86_64").expect("Intel macOS"),
            "x86_64-apple-darwin"
        );
        assert_eq!(
            target_triple("linux", "aarch64").expect("ARM Linux"),
            "aarch64-unknown-linux-gnu"
        );
        assert_eq!(
            target_triple("windows", "x86_64").expect("x64 Windows"),
            "x86_64-pc-windows-msvc"
        );
        assert!(target_triple("freebsd", "x86_64").is_err());
    }

    #[test]
    fn names_bare_release_binaries() {
        assert_eq!(
            asset_name("x86_64-unknown-linux-gnu", "linux"),
            "commit-wisp-x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            asset_name("aarch64-pc-windows-msvc", "windows"),
            "commit-wisp-aarch64-pc-windows-msvc.exe"
        );
    }

    #[test]
    fn accepts_only_v_prefixed_stable_release_versions() {
        assert_eq!(
            parse_release_version("v1.2.3")
                .expect("stable release")
                .to_string(),
            "1.2.3"
        );
        assert!(parse_release_version("1.2.3").is_err());
        assert!(parse_release_version("v1.2.3-beta.1").is_err());
        assert!(parse_release_version("latest").is_err());
    }

    #[test]
    fn updates_only_to_a_newer_version() {
        let current = Version::parse("1.2.3").expect("current version");
        assert!(update_available(
            &current,
            &Version::parse("1.3.0").expect("newer version")
        ));
        assert!(!update_available(
            &current,
            &Version::parse("1.2.3").expect("same version")
        ));
        assert!(!update_available(
            &current,
            &Version::parse("1.2.2").expect("older version")
        ));
    }

    #[test]
    fn checksum_match_is_exact_and_accepts_binary_marker() {
        let expected = "a".repeat(64);
        let sums = format!(
            "{}  commit-wisp-x86_64-unknown-linux-gnu.tar.gz\n{} *commit-wisp-x86_64-unknown-linux-gnu\n",
            "b".repeat(64),
            expected
        );
        assert_eq!(
            checksum_for(&sums, "commit-wisp-x86_64-unknown-linux-gnu").expect("matching checksum"),
            expected
        );
        assert!(checksum_for(&sums, "commit-wisp").is_err());
        assert!(checksum_for("invalid  commit-wisp\n", "commit-wisp").is_err());
    }

    #[test]
    fn detects_only_homebrew_cellar_installations() {
        assert!(is_homebrew_path(Path::new(
            "/opt/homebrew/Cellar/commit-wisp/1.2.3/bin/commit-wisp"
        )));
        assert!(is_homebrew_path(Path::new(
            "/home/linuxbrew/.linuxbrew/Cellar/commit-wisp/1.2.3/bin/commit-wisp"
        )));
        assert!(!is_homebrew_path(Path::new(
            "/Users/test/.local/bin/commit-wisp"
        )));
        assert!(!is_homebrew_path(Path::new(
            "/tmp/Cellar/another-formula/1.2.3/bin/commit-wisp"
        )));
    }

    #[tokio::test]
    async fn downloads_only_when_checksum_matches() {
        let asset_name = "commit-wisp-x86_64-unknown-linux-gnu";
        let binary = b"verified new binary";
        let checksum = format!("{:x}", Sha256::digest(binary));
        let (base_url, server) = update_server(asset_name, binary, &checksum);
        let destination = tempfile::tempdir().expect("update destination");
        let client = reqwest::Client::new();

        let file = download_verified(
            &client,
            &format!("{base_url}/{asset_name}"),
            &format!("{base_url}/SHA256SUMS"),
            asset_name,
            destination.path(),
        )
        .await
        .expect("verified download");

        assert_eq!(fs::read(file.path()).expect("read update"), binary);
        server.join().expect("update server");
    }

    #[tokio::test]
    async fn rejects_bad_checksum_before_writing_update_file() {
        let asset_name = "commit-wisp-x86_64-unknown-linux-gnu";
        let binary = b"tampered binary";
        let (base_url, server) = update_server(asset_name, binary, &"0".repeat(64));
        let destination = tempfile::tempdir().expect("update destination");
        let client = reqwest::Client::new();

        let error = download_verified(
            &client,
            &format!("{base_url}/{asset_name}"),
            &format!("{base_url}/SHA256SUMS"),
            asset_name,
            destination.path(),
        )
        .await
        .expect_err("checksum mismatch");

        assert!(error.to_string().contains("Checksum verification failed"));
        assert_eq!(
            fs::read_dir(destination.path())
                .expect("read destination")
                .count(),
            0
        );
        server.join().expect("update server");
    }
}
