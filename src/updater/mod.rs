//! Portable release updates. GitHub's HTTPS API is the trust anchor for asset digests.

pub(crate) mod install;

use anyhow::{Context, Result, bail, ensure};
use reqwest::{blocking::Client, redirect::Policy};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::Duration,
};

const REPOSITORY: &str = "Zain-Repo/relix";
const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
const MAX_EXPANDED: u64 = 1024 * 1024 * 1024;
const MAX_METADATA: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

#[derive(Clone, Debug)]
pub(crate) struct Release {
    pub version: String,
    pub notes: String,
    asset: Asset,
}

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
    state: String,
}

#[derive(Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    body: Option<String>,
    assets: Vec<Asset>,
}

/// Owns the staging directory until installation has copied its verified contents.
pub(crate) struct PreparedUpdate {
    pub staged_root: PathBuf,
    _directory: tempfile::TempDir,
}

fn platform_suffix() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok("windows-x64.zip"),
        ("linux", "x86_64") => Ok("linux-x64.tar.gz"),
        ("macos", "aarch64") => Ok("macos-arm64.tar.gz"),
        ("macos", "x86_64") => Ok("macos-x64.tar.gz"),
        _ => bail!("Automatic updates are unavailable for this platform."),
    }
}

fn client(timeout: Duration) -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!("Relix/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(timeout)
        .redirect(Policy::custom(|attempt| {
            // Release downloads redirect to GitHub's asset CDN. Never forward a request
            // to arbitrary hosts or accept an HTTPS-to-HTTP downgrade.
            let host = attempt.url().host_str().unwrap_or_default();
            if attempt.previous().len() < 5
                && attempt.url().scheme() == "https"
                && matches!(
                    host,
                    "github.com"
                        | "api.github.com"
                        | "release-assets.githubusercontent.com"
                        | "objects.githubusercontent.com"
                )
            {
                attempt.follow()
            } else {
                attempt.error("Untrusted release redirect")
            }
        }))
        .build()?)
}

fn stable_version(value: &str) -> Result<semver::Version> {
    let parsed = semver::Version::parse(value).context("Invalid release version")?;
    ensure!(
        parsed.pre.is_empty() && parsed.build.is_empty(),
        "Only stable releases are supported"
    );
    Ok(parsed)
}

fn asset_digest(asset: &Asset) -> Result<&str> {
    let digest = asset
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .context("Release has no GitHub SHA-256 digest; download refused")?;
    ensure!(
        digest.len() == 64 && digest.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid release digest"
    );
    Ok(digest)
}

fn select_release(
    response: ReleaseResponse,
    current: &str,
    suffix: &str,
) -> Result<Option<Release>> {
    ensure!(
        !response.draft && !response.prerelease,
        "Release is not stable and published"
    );
    let version = response
        .tag_name
        .strip_prefix('v')
        .context("Release tag must start with v")?;
    if stable_version(version)? <= stable_version(current)? {
        return Ok(None);
    }
    let expected_name = format!("Relix-{version}-{suffix}");
    let mut assets = response
        .assets
        .into_iter()
        .filter(|a| a.name == expected_name);
    let asset = assets
        .next()
        .context("Release does not include this platform")?;
    ensure!(
        assets.next().is_none(),
        "Release has duplicate platform assets"
    );
    ensure!(
        asset.state == "uploaded" && asset.size > 0 && asset.size <= MAX_ARCHIVE,
        "Invalid release asset size or state"
    );
    let expected_url =
        format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{expected_name}");
    ensure!(
        asset.browser_download_url == expected_url,
        "Release asset has an unexpected origin"
    );
    asset_digest(&asset)?;
    Ok(Some(Release {
        version: version.into(),
        notes: response.body.unwrap_or_default(),
        asset,
    }))
}

/// Checks only the fixed public repository, without sending local workspace data.
pub(crate) fn check() -> Result<Option<Release>> {
    let response = client(Duration::from_secs(30))?
        .get(format!(
            "https://api.github.com/repos/{REPOSITORY}/releases/latest"
        ))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .context("Could not reach the release service")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    response
        .error_for_status()
        .context("Release service rejected the check (possibly rate limited)")?
        .take(MAX_METADATA + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_METADATA,
        "Release metadata exceeds its size limit"
    );
    select_release(
        serde_json::from_slice(&bytes)?,
        env!("CARGO_PKG_VERSION"),
        platform_suffix()?,
    )
}

/// Downloads and verifies the package before extracting regular files into private staging.
pub(crate) fn download(release: &Release) -> Result<PreparedUpdate> {
    ensure!(
        stable_version(&release.version)? > stable_version(env!("CARGO_PKG_VERSION"))?,
        "Update must be newer than the running version"
    );
    let directory = tempfile::Builder::new()
        .prefix("relix-download-")
        .tempdir()?;
    let archive_path = directory.path().join("package");
    let mut archive = File::create(&archive_path)?;
    let response = client(Duration::from_secs(600))?
        .get(&release.asset.browser_download_url)
        .send()?
        .error_for_status()?;
    copy_verified(response, &mut archive, &release.asset)?;
    archive.sync_all()?;
    drop(archive);
    let extracted = directory.path().join("extracted");
    std::fs::create_dir(&extracted)?;
    extract(
        &archive_path,
        &extracted,
        release.asset.name.ends_with(".zip"),
    )?;
    let staged_root = extracted.join("Relix");
    let binary = if cfg!(target_os = "macos") {
        staged_root.join("Relix.app/Contents/MacOS/relix")
    } else {
        staged_root.join(if cfg!(windows) { "relix.exe" } else { "relix" })
    };
    ensure!(binary.is_file(), "Package is missing the Relix executable");
    Ok(PreparedUpdate {
        staged_root,
        _directory: directory,
    })
}

fn copy_verified(source: impl Read, archive: &mut impl Write, asset: &Asset) -> Result<()> {
    ensure!(
        asset.size > 0 && asset.size <= MAX_ARCHIVE,
        "Invalid release asset size"
    );
    let mut source = source.take(asset.size + 1);
    let mut hasher = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count += read as u64;
        ensure!(
            count <= asset.size,
            "Release download is larger than declared"
        );
        hasher.update(&buffer[..read]);
        archive.write_all(&buffer[..read])?;
    }
    ensure!(count == asset.size, "Release download is incomplete");
    ensure!(
        format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(asset_digest(asset)?),
        "Release checksum mismatch; installation refused"
    );
    Ok(())
}

fn safe_member(path: &Path) -> Result<()> {
    let mut components = path.components();
    ensure!(
        components.next() == Some(Component::Normal("Relix".as_ref())),
        "Archive must contain only the Relix package"
    );
    ensure!(
        components.all(|c| matches!(c, Component::Normal(_))),
        "Unsafe archive path"
    );
    // Reject Windows path and alternate-stream syntax even on Unix hosts.
    ensure!(
        !path.to_string_lossy().contains(['\\', ':']),
        "Unsafe archive filename"
    );
    Ok(())
}

fn extract(archive: &Path, destination: &Path, zip: bool) -> Result<()> {
    let mut expanded = 0u64;
    let mut names = std::collections::HashSet::new();
    if zip {
        let mut archive = zip::ZipArchive::new(File::open(archive)?)?;
        ensure!(archive.len() <= MAX_ENTRIES, "Too many archive entries");
        for i in 0..archive.len() {
            let entry = archive.by_index(i)?;
            let path = entry.enclosed_name().context("Unsafe ZIP path")?;
            safe_member(&path)?;
            ensure!(names.insert(path.clone()), "Duplicate archive path");
            let kind = entry.unix_mode().unwrap_or(0) & 0o170000;
            ensure!(
                matches!(kind, 0 | 0o040000 | 0o100000),
                "Archive links and special files are forbidden"
            );
            let target = destination.join(path);
            if entry.is_dir() {
                std::fs::create_dir_all(target)?;
                continue;
            }
            expanded = expanded
                .checked_add(entry.size())
                .context("Archive size overflow")?;
            ensure!(
                expanded <= MAX_EXPANDED,
                "Expanded archive exceeds size limit"
            );
            std::fs::create_dir_all(target.parent().context("Archive path has no parent")?)?;
            let mut output = File::options().write(true).create_new(true).open(target)?;
            let expected = entry.size();
            let copied = std::io::copy(&mut entry.take(expected + 1), &mut output)?;
            ensure!(copied == expected, "Archive entry size mismatch");
        }
    } else {
        let source = flate2::read::GzDecoder::new(File::open(archive)?);
        let mut archive = tar::Archive::new(source);
        for (i, entry) in archive.entries()?.enumerate() {
            ensure!(i < MAX_ENTRIES, "Too many archive entries");
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            safe_member(&path)?;
            ensure!(names.insert(path.clone()), "Duplicate archive path");
            let kind = entry.header().entry_type();
            ensure!(
                kind.is_file() || kind.is_dir(),
                "Archive links and special files are forbidden"
            );
            let target = destination.join(&path);
            if kind.is_dir() {
                std::fs::create_dir_all(target)?;
                continue;
            }
            let size = entry.size();
            expanded = expanded
                .checked_add(size)
                .context("Archive size overflow")?;
            ensure!(
                expanded <= MAX_EXPANDED,
                "Expanded archive exceeds size limit"
            );
            std::fs::create_dir_all(target.parent().context("Archive path has no parent")?)?;
            let mut output = File::options().write(true).create_new(true).open(&target)?;
            let copied = std::io::copy(&mut entry, &mut output)?;
            ensure!(copied == size, "Archive entry size mismatch");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                // Preserve executable bits while stripping privileged/group-write modes.
                std::fs::set_permissions(
                    &target,
                    std::fs::Permissions::from_mode(entry.header().mode()? & 0o755),
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(version: &str) -> ReleaseResponse {
        let name = format!("Relix-{version}-windows-x64.zip");
        ReleaseResponse {
            tag_name: format!("v{version}"),
            draft: false,
            prerelease: false,
            body: Some("Notes".into()),
            assets: vec![Asset {
                name: name.clone(),
                size: 32,
                digest: Some(format!("sha256:{}", "a".repeat(64))),
                state: "uploaded".into(),
                browser_download_url: format!(
                    "https://github.com/{REPOSITORY}/releases/download/v{version}/{name}"
                ),
            }],
        }
    }

    #[test]
    fn release_selection_supports_skipped_versions_and_refuses_downgrades() {
        assert!(
            select_release(response("1.4.0"), "0.1.1", "windows-x64.zip")
                .unwrap()
                .is_some()
        );
        assert!(
            select_release(response("0.1.1"), "0.1.1", "windows-x64.zip")
                .unwrap()
                .is_none()
        );
        assert!(
            select_release(response("0.1.0"), "0.1.1", "windows-x64.zip")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn release_selection_refuses_unverified_or_foreign_assets() {
        let mut missing = response("0.2.0");
        missing.assets[0].digest = None;
        assert!(select_release(missing, "0.1.1", "windows-x64.zip").is_err());
        let mut foreign = response("0.2.0");
        foreign.assets[0].browser_download_url = "https://example.com/update.zip".into();
        assert!(select_release(foreign, "0.1.1", "windows-x64.zip").is_err());
        let mut duplicate = response("0.2.0");
        duplicate.assets.push(duplicate.assets[0].clone());
        assert!(select_release(duplicate, "0.1.1", "windows-x64.zip").is_err());
    }

    #[test]
    fn archive_paths_refuse_escape_and_windows_streams() {
        for name in [
            "../relix",
            "/Relix/relix",
            "Relix/../relix",
            "Relix/C:evil",
            "Relix/a\\b",
            "Other/relix",
        ] {
            assert!(safe_member(Path::new(name)).is_err(), "{name}");
        }
        assert!(safe_member(Path::new("Relix/relix.exe")).is_ok());
    }

    #[test]
    fn extract_rejects_tar_symlinks_without_writing_outside_staging() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("archive");
        let encoder = flate2::write::GzEncoder::new(
            File::create(&archive).unwrap(),
            flate2::Compression::default(),
        );
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        tar.append_link(&mut header, "Relix/evil", "../../outside")
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        let staging = dir.path().join("staging");
        std::fs::create_dir(&staging).unwrap();
        assert!(extract(&archive, &staging, false).is_err());
        assert!(!dir.path().join("outside").exists());
    }

    #[test]
    fn download_verification_refuses_corruption_truncation_and_extra_bytes() {
        let bytes = b"verified package";
        let mut asset = response("0.2.0").assets.remove(0);
        asset.size = bytes.len() as u64;
        asset.digest = Some(format!("sha256:{:x}", Sha256::digest(bytes)));
        let mut output = Vec::new();
        copy_verified(&bytes[..], &mut output, &asset).unwrap();
        assert_eq!(output, bytes);
        for source in [
            b"corrupt! package".as_slice(),
            &bytes[..bytes.len() - 1],
            b"verified package!",
        ] {
            assert!(copy_verified(source, &mut Vec::new(), &asset).is_err());
        }
    }

    #[test]
    fn zip_extraction_preserves_expected_package_and_rejects_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("package.zip");
        let mut writer = zip::ZipWriter::new(File::create(&path).unwrap());
        writer
            .start_file("Relix/relix.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"package executable").unwrap();
        writer.finish().unwrap();
        let output = dir.path().join("output");
        std::fs::create_dir(&output).unwrap();
        extract(&path, &output, true).unwrap();
        assert_eq!(
            std::fs::read(output.join("Relix/relix.exe")).unwrap(),
            b"package executable"
        );
        // Existing files are never overwritten, even by a second extraction.
        assert!(extract(&path, &output, true).is_err());
    }

    #[test]
    fn prereleases_and_missing_platform_packages_are_rejected() {
        let mut prerelease = response("0.2.0");
        prerelease.prerelease = true;
        assert!(select_release(prerelease, "0.1.1", "windows-x64.zip").is_err());
        assert!(select_release(response("0.2.0"), "0.1.1", "linux-x64.tar.gz").is_err());
        assert!(stable_version("0.2.0-rc.1").is_err());
    }
}
