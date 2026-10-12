//! Restart-time installation with per-file backups and bounded process waits.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const HELPER_FLAG: &str = "--relix-update-helper";
const WAIT_LIMIT: Duration = Duration::from_secs(120);
const PROBE_LIMIT: Duration = Duration::from_secs(15);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PayloadFile {
    relative: PathBuf,
    digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallPlan {
    pid: u32,
    destination: PathBuf,
    executable: PathBuf,
    original_digest: String,
    version: String,
    files: Vec<PayloadFile>,
}

/// Launches the copied helper; the caller must then close the application normally.
/// The supplied payload must already match the trusted GitHub release asset digest.
pub fn launch_install(staged_root: &Path, version: &str) -> Result<()> {
    ensure!(
        !version.is_empty() && version.len() < 128,
        "Invalid update version"
    );
    let current = std::env::current_exe()?.canonicalize()?;
    let (destination, executable) = installation_layout(&current)?;
    #[cfg(target_os = "macos")]
    let staged = staged_root.join("Relix.app").canonicalize()?;
    #[cfg(not(target_os = "macos"))]
    let staged = staged_root.canonicalize()?;
    let files = payload_files(&staged)?;
    ensure!(
        files.iter().any(|file| file.relative == executable),
        "Update executable is missing"
    );
    let helper_dir = tempfile::Builder::new()
        .prefix(".relix-update-")
        .tempdir_in(
            destination
                .parent()
                .context("Installation directory has no parent")?,
        )?;
    let root = helper_dir.path();
    fs::create_dir(root.join("payload"))?;
    for file in &files {
        prepare_parent(&root.join("payload"), &file.relative)?;
        copy_regular(
            &staged.join(&file.relative),
            &root.join("payload").join(&file.relative),
        )?;
    }
    let helper = root.join(helper_name());
    copy_regular(&current, &helper)?;
    #[cfg(windows)]
    for name in CRT_FILES {
        let library = destination.join(name);
        if library.exists() {
            copy_regular(&library, &root.join(name))?;
        }
    }
    let plan = InstallPlan {
        pid: std::process::id(),
        destination,
        executable,
        original_digest: digest_file(&current)?,
        version: version.to_owned(),
        files,
    };
    let plan_path = root.join("plan.json");
    let mut plan_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&plan_path)?;
    plan_file.write_all(&serde_json::to_vec(&plan)?)?;
    plan_file.sync_all()?;
    let log = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("result.log"))?;
    let mut command = Command::new(&helper);
    command
        .arg(HELPER_FLAG)
        .arg(&plan_path)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    hide_window(&mut command);
    let mut child = command.spawn().context("Could not start update helper")?;
    // Retain recovery artifacts for inspection and manual recovery after the app exits.
    let retained = helper_dir.keep();
    wait_for_readiness(&mut child, &retained, Duration::from_secs(10))?;
    Ok(())
}

fn wait_for_readiness(child: &mut std::process::Child, root: &Path, limit: Duration) -> Result<()> {
    let deadline = Instant::now() + limit;
    let readiness = (|| -> Result<()> {
        while !root.join("ready").try_exists()? {
            ensure!(
                child.try_wait()?.is_none(),
                "Update helper exited before accepting the plan; see {}",
                root.join("result.log").display()
            );
            ensure!(
                Instant::now() < deadline,
                "Update helper did not accept the plan in time; application will remain open"
            );
            thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    })();
    if let Err(error) = readiness {
        // A rejected launch must never leave a helper that could install on a later ordinary exit.
        child
            .kill()
            .context("Could not stop the rejected update helper")?;
        child
            .wait()
            .context("Could not reap the rejected update helper")?;
        return Err(error);
    }
    Ok(())
}

/// Handles the private helper CLI before the GPUI runtime is initialized.
pub fn run_helper_if_requested() -> Result<bool> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(HELPER_FLAG)) {
        return Ok(false);
    }
    let plan_path = PathBuf::from(args.next().context("Missing update plan")?);
    ensure!(args.next().is_none(), "Unexpected helper arguments");
    let helper = std::env::current_exe()?.canonicalize()?;
    let root = helper.parent().context("Missing helper directory")?;
    ensure!(
        helper.file_name() == Some(std::ffi::OsStr::new(helper_name())),
        "Invalid update helper name"
    );
    ensure!(
        plan_path.canonicalize()? == root.join("plan.json"),
        "Invalid update plan location"
    );
    ensure!(
        fs::metadata(&plan_path)?.len() < 1024 * 1024,
        "Update plan is too large"
    );
    let plan: InstallPlan = serde_json::from_slice(&fs::read(&plan_path)?)?;
    validate_plan(&plan, root, &helper)?;
    fs::write(root.join("ready"), b"ready")?;
    wait_for_process(plan.pid, WAIT_LIMIT)?;
    ensure!(
        digest_file(&plan.destination.join(&plan.executable))? == plan.original_digest,
        "Installed application changed while waiting for exit"
    );
    let result = install_payload(&plan, root, || {
        probe_version(&plan.destination.join(&plan.executable), &plan.version)
    });
    if let Err(error) = result {
        let _ = fs::write(root.join("failure.txt"), format!("{error:#}"));
        // A recovered installation should reopen so the user is not left without their workbench.
        if error.to_string() == "Update failed; previous installation was restored" {
            let _ = launch_application(&plan.destination.join(&plan.executable));
        }
        return Err(error);
    }
    fs::write(root.join("success.txt"), &plan.version)?;
    launch_application(&plan.destination.join(&plan.executable))?;
    Ok(true)
}

fn helper_name() -> &'static str {
    if cfg!(windows) {
        "relix-update-helper.exe"
    } else {
        "relix-update-helper"
    }
}

const CRT_FILES: &[&str] = &[
    "concrt140.dll",
    "msvcp140.dll",
    "msvcp140_1.dll",
    "msvcp140_2.dll",
    "msvcp140_atomic_wait.dll",
    "msvcp140_codecvt_ids.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "vcruntime140_threads.dll",
    "vccorlib140.dll",
];

fn installation_layout(current: &Path) -> Result<(PathBuf, PathBuf)> {
    #[cfg(target_os = "macos")]
    {
        let bundle = current
            .ancestors()
            .find(|path| path.extension().is_some_and(|ext| ext == "app"))
            .context("Automatic updates require an installed Relix.app bundle")?;
        ensure!(
            bundle.file_name() == Some(std::ffi::OsStr::new("Relix.app")),
            "Unexpected application bundle"
        );
        Ok((
            bundle.to_path_buf(),
            current.strip_prefix(bundle)?.to_path_buf(),
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let expected = if cfg!(windows) { "relix.exe" } else { "relix" };
        ensure!(
            current.file_name() == Some(std::ffi::OsStr::new(expected)),
            "Automatic updates require a release installation"
        );
        Ok((
            current
                .parent()
                .context("Missing installation directory")?
                .to_path_buf(),
            PathBuf::from(expected),
        ))
    }
}

fn validate_plan(plan: &InstallPlan, root: &Path, helper: &Path) -> Result<()> {
    ensure!(
        plan.pid > 1 && plan.pid != std::process::id(),
        "Invalid application process"
    );
    ensure!(
        plan.destination.canonicalize()? == plan.destination,
        "Installation must be canonical"
    );
    ensure!(
        root.parent() == plan.destination.parent(),
        "Helper must be beside the installation"
    );
    ensure!(
        !root.starts_with(&plan.destination),
        "Helper is inside installation"
    );
    let original = plan.destination.join(&plan.executable);
    ensure!(
        installation_layout(&original)? == (plan.destination.clone(), plan.executable.clone()),
        "Unexpected installation layout"
    );
    ensure!(
        digest_file(helper)? == plan.original_digest,
        "Helper integrity check failed"
    );
    ensure!(
        digest_file(&original)? == plan.original_digest,
        "Installed application changed since update was requested"
    );
    let mut names = HashSet::new();
    ensure!(
        !plan.files.is_empty() && plan.files.len() <= 10_000,
        "Invalid payload file count"
    );
    for file in &plan.files {
        validate_relative(&file.relative)?;
        ensure!(names.insert(&file.relative), "Duplicate update file");
    }
    ensure!(
        names.contains(&plan.executable),
        "Update executable is missing"
    );
    verify_payload(plan, root)
}

fn validate_relative(path: &Path) -> Result<()> {
    ensure!(
        !path.as_os_str().is_empty()
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "Unsafe payload path"
    );
    #[cfg(not(target_os = "macos"))]
    {
        ensure!(
            path.components().count() == 1,
            "Unexpected payload directory"
        );
        let name = path.to_str().context("Invalid payload filename")?;
        let allowed = if cfg!(windows) {
            name == "relix.exe" || CRT_FILES.contains(&name)
        } else {
            name == "relix"
        };
        ensure!(allowed, "Unexpected update file: {name}");
    }
    #[cfg(target_os = "macos")]
    ensure!(path.starts_with("Contents"), "Unexpected bundle payload");
    Ok(())
}

fn payload_files(root: &Path) -> Result<Vec<PayloadFile>> {
    fn walk(root: &Path, directory: &Path, files: &mut Vec<PayloadFile>) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            ensure!(!is_link(&metadata), "Update payload contains a symlink");
            if metadata.is_dir() {
                walk(root, &entry.path(), files)?;
            } else {
                ensure!(metadata.is_file(), "Update payload contains a special file");
                let relative = entry.path().strip_prefix(root)?.to_path_buf();
                if matches!(
                    relative.to_str(),
                    Some("README.md" | "RELEASE-NOTES.md" | "RELEASE-PROCESS.md" | "UPDATES.md")
                ) {
                    continue;
                }
                validate_relative(&relative)?;
                files.push(PayloadFile {
                    relative,
                    digest: digest_file(&entry.path())?,
                });
                ensure!(files.len() <= 10_000, "Too many update files");
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

fn digest_file(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !is_link(&metadata),
        "Expected a regular file"
    );
    let mut reader = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_regular(source: &Path, destination: &Path) -> Result<()> {
    digest_file(source)?;
    let mut source_file = File::open(source)?;
    // Atomic exclusive creation rejects existing files, including dangling links.
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    std::io::copy(&mut source_file, &mut destination_file)?;
    destination_file.set_permissions(source_file.metadata()?.permissions())?;
    destination_file.sync_all()?;
    Ok(())
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Reject junctions and other reparse points as well as symbolic links.
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn prepare_parent(root: &Path, relative: &Path) -> Result<()> {
    let root_metadata = fs::symlink_metadata(root)?;
    ensure!(
        root_metadata.is_dir() && !is_link(&root_metadata),
        "Unsafe installation directory"
    );
    let mut directory = root.to_path_buf();
    for component in relative
        .parent()
        .context("Missing file parent")?
        .components()
    {
        ensure!(
            matches!(component, Component::Normal(_)),
            "Unsafe installation path"
        );
        directory.push(component.as_os_str());
        match fs::symlink_metadata(&directory) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && !is_link(&metadata),
                "Installation parent is not a regular directory"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&directory)?;
                let metadata = fs::symlink_metadata(&directory)?;
                ensure!(
                    metadata.is_dir() && !is_link(&metadata),
                    "Unsafe installation parent"
                );
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn verify_payload(plan: &InstallPlan, root: &Path) -> Result<()> {
    let payload = root.join("payload").canonicalize()?;
    for file in &plan.files {
        let path = payload.join(&file.relative);
        ensure!(
            path.canonicalize()?.starts_with(&payload),
            "Payload escapes staging directory"
        );
        ensure!(
            digest_file(&path)? == file.digest,
            "Update payload integrity check failed"
        );
    }
    Ok(())
}

fn install_payload(
    plan: &InstallPlan,
    root: &Path,
    probe: impl FnOnce() -> Result<()>,
) -> Result<()> {
    verify_payload(plan, root)?;
    let backup = root.join("backup");
    fs::create_dir(&backup).context("Update backup already exists or cannot be created")?;
    let mut replaced = Vec::new();
    let attempt = (|| {
        for file in &plan.files {
            let destination = plan.destination.join(&file.relative);
            prepare_parent(&plan.destination, &file.relative)?;
            let existed = match fs::symlink_metadata(&destination) {
                Ok(metadata) => {
                    ensure!(
                        metadata.is_file() && !is_link(&metadata),
                        "Installation target is not a regular file"
                    );
                    true
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(error.into()),
            };
            if existed {
                digest_file(&destination)?;
                let saved = backup.join(&file.relative);
                prepare_parent(&backup, &file.relative)?;
                fs::rename(&destination, &saved)?;
            }
            replaced.push((file.relative.clone(), existed));
            copy_regular(&root.join("payload").join(&file.relative), &destination)?;
            ensure!(
                digest_file(&destination)? == file.digest,
                "Installed file integrity check failed"
            );
        }
        probe()
    })();
    if let Err(error) = attempt {
        let mut failures = Vec::new();
        for (relative, existed) in replaced.into_iter().rev() {
            let destination = plan.destination.join(&relative);
            let restore = (|| -> Result<()> {
                prepare_parent(&plan.destination, &relative)?;
                match fs::symlink_metadata(&destination) {
                    Ok(_) => fs::remove_file(&destination)?,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                if existed {
                    fs::rename(backup.join(&relative), destination)?;
                }
                Ok(())
            })();
            if let Err(restore_error) = restore {
                failures.push(format!("{}: {restore_error}", relative.display()));
            }
        }
        ensure!(
            failures.is_empty(),
            "Update failed ({error:#}); recovery requires backup {}: {}",
            backup.display(),
            failures.join("; ")
        );
        return Err(error.context("Update failed; previous installation was restored"));
    }
    Ok(())
}

fn probe_version(executable: &Path, version: &str) -> Result<()> {
    let mut command = Command::new(executable);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_window(&mut command);
    let mut child = command
        .spawn()
        .context("Updated executable could not start")?;
    let deadline = Instant::now() + PROBE_LIMIT;
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Updated executable version probe failed");
            let mut output = String::new();
            child
                .stdout
                .take()
                .context("Missing version output")?
                .take(1024)
                .read_to_string(&mut output)?;
            ensure!(
                output.trim() == format!("Relix {version}"),
                "Updated executable reported the wrong version"
            );
            return Ok(());
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            bail!("Updated executable version probe timed out");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn launch_application(executable: &Path) -> Result<()> {
    let mut command = Command::new(executable);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_window(&mut command);
    command.spawn().context("Could not restart Relix")?;
    Ok(())
}

fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

#[cfg(windows)]
fn wait_for_process(pid: u32, limit: Duration) -> Result<()> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    // SAFETY: Only SYNCHRONIZE access is requested, and the owned handle stays open during the bounded wait.
    let handle = unsafe { OpenProcess(0x00100000, 0, pid) };
    if handle.is_null() {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(87) {
            return Ok(());
        }
        return Err(error.into());
    }
    // SAFETY: OpenProcess returned a valid process handle; no other code can close it.
    let result =
        unsafe { WaitForSingleObject(handle, limit.as_millis().min(u32::MAX as u128) as u32) };
    // SAFETY: This closes our owned handle once, after waiting has finished.
    unsafe { CloseHandle(handle) };
    ensure!(
        result == 0,
        "Waiting for Relix to exit failed or timed out (code {result})"
    );
    Ok(())
}

#[cfg(unix)]
fn wait_for_process(pid: u32, limit: Duration) -> Result<()> {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    let pid = i32::try_from(pid).context("Invalid process identifier")?;
    ensure!(pid > 1, "Invalid process identifier");
    let deadline = Instant::now() + limit;
    loop {
        // SAFETY: Signal zero only checks process existence; it never sends a terminating signal.
        if unsafe { kill(pid, 0) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(3) {
                return Ok(());
            }
            return Err(error.into());
        }
        ensure!(
            Instant::now() < deadline,
            "Waiting for Relix to exit timed out"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, InstallPlan) {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("installed");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(directory.path().join("payload")).unwrap();
        let executable = PathBuf::from(if cfg!(target_os = "macos") {
            "Contents/MacOS/relix"
        } else if cfg!(windows) {
            "relix.exe"
        } else {
            "relix"
        });
        prepare_parent(&destination, &executable).unwrap();
        prepare_parent(&directory.path().join("payload"), &executable).unwrap();
        fs::write(destination.join(&executable), b"old executable").unwrap();
        fs::write(destination.join("user.db"), b"user data").unwrap();
        fs::write(
            directory.path().join("payload").join(&executable),
            b"new executable",
        )
        .unwrap();
        let digest = digest_file(&directory.path().join("payload").join(&executable)).unwrap();
        let plan = InstallPlan {
            pid: 5,
            destination: destination.canonicalize().unwrap(),
            executable: executable.clone(),
            original_digest: String::new(),
            version: "0.2.0".into(),
            files: vec![PayloadFile {
                relative: executable,
                digest,
            }],
        };
        (directory, plan)
    }

    #[test]
    fn installation_preserves_unmanaged_user_files() {
        let (root, plan) = fixture();
        install_payload(&plan, root.path(), || Ok(())).unwrap();
        assert_eq!(
            fs::read(plan.destination.join("user.db")).unwrap(),
            b"user data"
        );
        assert_eq!(
            fs::read(plan.destination.join(plan.executable)).unwrap(),
            b"new executable"
        );
    }

    #[test]
    fn failed_probe_restores_previous_executable() {
        let (root, plan) = fixture();
        assert!(install_payload(&plan, root.path(), || bail!("bad version")).is_err());
        assert_eq!(
            fs::read(plan.destination.join(plan.executable)).unwrap(),
            b"old executable"
        );
    }

    #[test]
    fn modified_payload_is_rejected_before_replacement() {
        let (root, plan) = fixture();
        fs::write(
            root.path().join("payload").join(&plan.executable),
            b"tampered",
        )
        .unwrap();
        assert!(install_payload(&plan, root.path(), || Ok(())).is_err());
        assert_eq!(
            fs::read(plan.destination.join(plan.executable)).unwrap(),
            b"old executable"
        );
    }

    #[test]
    fn package_documentation_is_not_installed_over_user_files() {
        let (root, _) = fixture();
        fs::write(root.path().join("payload/README.md"), b"release readme").unwrap();
        let files = payload_files(&root.path().join("payload")).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn unexpected_package_file_is_rejected() {
        let (root, _) = fixture();
        fs::write(root.path().join("payload/user.db"), b"unexpected data").unwrap();
        assert!(payload_files(&root.path().join("payload")).is_err());
    }

    #[test]
    fn copy_refuses_to_overwrite_existing_destination() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::write(&source, b"replacement").unwrap();
        fs::write(&destination, b"existing").unwrap();
        assert!(copy_regular(&source, &destination).is_err());
        assert_eq!(fs::read(destination).unwrap(), b"existing");
    }

    #[test]
    fn parent_validation_rejects_file_before_creating_descendants() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Contents"), b"ordinary file").unwrap();
        assert!(prepare_parent(root.path(), Path::new("Contents/MacOS/relix")).is_err());
        assert_eq!(
            fs::read(root.path().join("Contents")).unwrap(),
            b"ordinary file"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_destination_link_is_rejected_without_writing_outside() {
        let (root, plan) = fixture();
        let destination = plan.destination.join(&plan.executable);
        fs::remove_file(&destination).unwrap();
        let outside = root.path().join("outside");
        std::os::unix::fs::symlink(&outside, &destination).unwrap();
        assert!(install_payload(&plan, root.path(), || Ok(())).is_err());
        assert!(!outside.exists());
        assert!(
            fs::symlink_metadata(destination)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn linked_parent_is_rejected_before_creating_outside_directories() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("Contents")).unwrap();
        assert!(prepare_parent(root.path(), Path::new("Contents/MacOS/relix")).is_err());
        assert!(!outside.path().join("MacOS").exists());
    }

    #[cfg(windows)]
    #[test]
    fn junction_parent_is_rejected_before_creating_outside_directories() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let junction = root.path().join("Contents");
        let mut command = Command::new("cmd.exe");
        command
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(outside.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_window(&mut command);
        assert!(command.status().unwrap().success());
        assert!(prepare_parent(root.path(), Path::new("Contents/MacOS/relix")).is_err());
        assert!(!outside.path().join("MacOS").exists());
        fs::remove_dir(junction).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn later_copy_failure_rolls_back_earlier_replacement() {
        let (root, mut plan) = fixture();
        let relative = PathBuf::from("vcruntime140.dll");
        let source = root.path().join("payload").join(&relative);
        fs::write(&source, b"new runtime").unwrap();
        fs::create_dir(plan.destination.join(&relative)).unwrap();
        plan.files.push(PayloadFile {
            relative,
            digest: digest_file(&source).unwrap(),
        });
        assert!(install_payload(&plan, root.path(), || Ok(())).is_err());
        assert_eq!(
            fs::read(plan.destination.join(plan.executable)).unwrap(),
            b"old executable"
        );
    }

    #[test]
    fn traversal_and_unmanaged_paths_are_rejected() {
        assert!(validate_relative(Path::new("../relix.exe")).is_err());
        assert!(validate_relative(Path::new("user.db")).is_err());
    }

    #[test]
    fn running_process_wait_is_bounded() {
        assert!(wait_for_process(std::process::id(), Duration::from_millis(1)).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn readiness_timeout_terminates_helper() {
        let root = tempfile::tempdir().unwrap();
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ]);
        hide_window(&mut command);
        let mut child = command.spawn().unwrap();
        assert!(wait_for_readiness(&mut child, root.path(), Duration::ZERO).is_err());
        assert!(child.try_wait().unwrap().is_some());
    }
}
