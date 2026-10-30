use eframe::egui;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
    ptr,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, WAIT_OBJECT_0},
    Storage::FileSystem::ReplaceFileW,
    System::Threading::{
        CREATE_NO_WINDOW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        QueryFullProcessImageNameW, WaitForSingleObject,
    },
};

const REPO: &str = "https://github.com/siarheisinelnikau/multi-monitor-brightness";
const API: &str =
    "https://api.github.com/repos/siarheisinelnikau/multi-monitor-brightness/releases/latest";
const STAGED_EXE: &str = "MultiMonitorBrightness.exe";
const MAX_EXE: u64 = 64 * 1024 * 1024;
const STAGE_PREFIX: &str = ".mmb-update-";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub notes: String,
    pub page: String,
    executable: Asset,
    checksum: Asset,
}

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

pub enum Event {
    Checked(Result<Option<Box<Release>>, String>),
    Progress { received: u64, total: u64 },
    Ready(Result<Prepared, String>),
}
enum Request {
    Check,
    Download(Box<Release>),
    Stop,
}

pub struct Service {
    sender: mpsc::Sender<Request>,
    pub events: mpsc::Receiver<Event>,
}

impl Service {
    pub fn new(ctx: egui::Context, portable: bool) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let (out, events) = mpsc::channel();
        thread::Builder::new()
            .name("github-updates".into())
            .spawn(move || {
                let send = |event| {
                    let _ = out.send(event);
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                };
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::Check => send(Event::Checked(
                            check_latest(crate::VERSION).map(|release| release.map(Box::new)),
                        )),
                        Request::Download(release) => send(Event::Ready(prepare(
                            &release,
                            portable,
                            |received, total| send(Event::Progress { received, total }),
                        ))),
                        Request::Stop => break,
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { sender, events })
    }
    #[cfg(test)]
    pub fn test_channel() -> (Self, impl Fn() -> usize) {
        let (sender, requests) = mpsc::channel();
        let (_, events) = mpsc::channel();
        (Self { sender, events }, move || requests.try_iter().count())
    }

    pub fn check(&self) {
        let _ = self.sender.send(Request::Check);
    }
    pub fn download(&self, release: Box<Release>) {
        let _ = self.sender.send(Request::Download(release));
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.sender.send(Request::Stop);
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(5)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(120)))
        .timeout_per_read(Some(Duration::from_secs(20)))
        .user_agent(format!("MultiMonitorBrightness/{}", crate::VERSION))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent()
}

fn read_text(agent: &ureq::Agent, url: &str, limit: u64) -> Result<String, String> {
    let mut response = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(network_error)?;
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_string()
        .map_err(network_error)
}

fn network_error(e: ureq::Error) -> String {
    if matches!(e, ureq::Error::StatusCode(403 | 429)) {
        "GitHub's request limit was reached. Try again later.".into()
    } else {
        format!("Could not contact GitHub: {e}")
    }
}

fn check_latest(current: &str) -> Result<Option<Release>, String> {
    parse_release(&read_text(&agent(), API, 1024 * 1024)?, current)
}

fn parse_release(json: &str, current: &str) -> Result<Option<Release>, String> {
    let release: ApiRelease = serde_json::from_str(json)
        .map_err(|e| format!("GitHub returned an invalid release: {e}"))?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let version = Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )
    .map_err(|_| "The release version is invalid.")?;
    if !version.pre.is_empty() || version <= Version::parse(current).map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let tag = format!("v{version}");
    if release.tag_name != tag || release.html_url != format!("{REPO}/releases/tag/{tag}") {
        return Err("The release does not belong to the expected GitHub repository.".into());
    }
    let asset = |name: &str| -> Result<Asset, String> {
        let mut matches = release.assets.iter().filter(|a| a.name == name);
        let asset = matches
            .next()
            .ok_or_else(|| format!("The release is missing {name}."))?;
        if matches.next().is_some()
            || asset.browser_download_url != format!("{REPO}/releases/download/{tag}/{name}")
        {
            return Err("The release contains an unexpected download address.".into());
        }
        Ok(asset.clone())
    };
    let executable = asset(&format!("MultiMonitorBrightness-v{version}.exe"))?;
    if executable.size < 1024 || executable.size > MAX_EXE {
        return Err("The release executable has an unexpected size.".into());
    }
    Ok(Some(Release {
        version,
        notes: release
            .body
            .unwrap_or_default()
            .chars()
            .take(12000)
            .collect(),
        page: release.html_url,
        executable,
        checksum: asset("SHA256SUMS.txt")?,
    }))
}

fn checksum(text: &str, executable_name: &str) -> Result<String, String> {
    let mut found = None;
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() == 2 && fields[1].trim_start_matches('*') == executable_name {
            if found.is_some()
                || fields[0].len() != 64
                || !fields[0].bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("The release checksum file is invalid.".into());
            }
            found = Some(fields[0].to_ascii_lowercase());
        }
    }
    found.ok_or_else(|| "The release checksum file does not include the executable.".into())
}

fn hash(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn verify_executable(path: &Path, expected: &str, size: u64) -> Result<(), String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() != size || hash(path)? != expected {
        return Err(
            "The downloaded executable failed SHA-256 verification. Nothing was installed.".into(),
        );
    }
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut header = [0u8; 64];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    let offset = u32::from_le_bytes(header[60..64].try_into().unwrap()) as u64;
    if &header[..2] != b"MZ" || offset > size.saturating_sub(6) {
        return Err("The download is not a Windows executable.".into());
    }
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut pe = [0u8; 6];
    file.read_exact(&mut pe).map_err(|e| e.to_string())?;
    if &pe[..4] != b"PE\0\0" || pe[4..6] != [0x64, 0x86] {
        return Err("The download is not a Windows x64 executable.".into());
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Plan {
    target: PathBuf,
    parent_pid: u32,
    original_sha: String,
    new_sha: String,
    version: String,
    portable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    smoke_marker: Option<PathBuf>,
}

pub struct Prepared {
    directory: PathBuf,
    handed_off: bool,
}
impl Drop for Prepared {
    fn drop(&mut self) {
        if !self.handed_off
            && let Some(parent) = self.directory.parent()
        {
            cleanup(&self.directory, parent);
        }
    }
}

fn new_stage(parent: &Path) -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = parent.join(format!("{STAGE_PREFIX}{}-{stamp}", std::process::id()));
    fs::create_dir(&path)
        .map_err(|e| format!("The executable's folder must be writable to update in place: {e}"))?;
    Ok(path)
}

fn prepare(
    release: &Release,
    portable: bool,
    mut progress: impl FnMut(u64, u64),
) -> Result<Prepared, String> {
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = target
        .parent()
        .ok_or("The executable folder is unavailable.")?
        .to_path_buf();
    let directory = new_stage(&parent)?;
    let result = (|| {
        let agent = agent();
        let expected = checksum(
            &read_text(&agent, &release.checksum.browser_download_url, 128 * 1024)?,
            &release.executable.name,
        )?;
        if let Some(digest) = &release.executable.digest
            && digest != &format!("sha256:{expected}")
        {
            return Err("GitHub's digest and checksum file do not agree.".into());
        }
        let path = directory.join(STAGED_EXE);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let mut response = agent
            .get(&release.executable.browser_download_url)
            .call()
            .map_err(network_error)?;
        let mut reader = response.body_mut().as_reader();
        let mut buffer = [0u8; 64 * 1024];
        let mut received = 0;
        let mut notified = Instant::now();
        loop {
            let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            received += n as u64;
            if received > release.executable.size || received > MAX_EXE {
                return Err("The download exceeded the expected size.".into());
            }
            file.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            if notified.elapsed() > Duration::from_millis(100) {
                progress(received, release.executable.size);
                notified = Instant::now();
            }
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        verify_executable(&path, &expected, release.executable.size)?;
        let plan = Plan {
            original_sha: hash(&target)?,
            target,
            parent_pid: std::process::id(),
            new_sha: expected,
            version: release.version.to_string(),
            portable,
            smoke_marker: None,
        };
        fs::write(
            directory.join("update.json"),
            serde_json::to_vec(&plan).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        progress(received, received);
        Ok(Prepared {
            directory: directory.clone(),
            handed_off: false,
        })
    })();
    if result.is_err() {
        cleanup(&directory, &parent);
    }
    result
}

pub fn handoff(prepared: &mut Prepared) -> Result<(), String> {
    Command::new(prepared.directory.join(STAGED_EXE))
        .arg("--apply-update")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Could not start the update helper: {e}"))?;
    prepared.handed_off = true;
    Ok(())
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn replace(target: &Path, ready: &Path, backup: &Path) -> Result<(), String> {
    let (target, ready, backup) = (wide_path(target), wide_path(ready), wide_path(backup));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        // SAFETY: All paths are validated, same-volume, NUL-terminated and live across this call.
        if unsafe {
            ReplaceFileW(
                target.as_ptr(),
                ready.as_ptr(),
                backup.as_ptr(),
                0,
                ptr::null(),
                ptr::null(),
            )
        } != 0
        {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if Instant::now() >= deadline {
            return Err(format!("Could not replace the executable: {error}"));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn wait_for_parent(plan: &Plan) -> Result<(), String> {
    // SAFETY: Open only the specified application process for querying and waiting, never termination.
    let process = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            plan.parent_pid,
        )
    };
    if process.is_null() {
        if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
            return Ok(());
        }
        return Err("The application exited before the update helper could attach. Please retry the update.".into());
    }
    let result = (|| {
        // A terminated process may still have a valid handle but no queryable image name.
        if unsafe { WaitForSingleObject(process, 0) } == WAIT_OBJECT_0 {
            return Ok(());
        }
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        if unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) } == 0 {
            if unsafe { WaitForSingleObject(process, 0) } == WAIT_OBJECT_0 {
                return Ok(());
            }
            return Err(format!(
                "Could not verify the application being updated: {}",
                std::io::Error::last_os_error()
            ));
        }
        let path =
            PathBuf::from(String::from_utf16(&path[..length as usize]).map_err(|e| e.to_string())?);
        if fs::canonicalize(path).map_err(|e| e.to_string())?
            != fs::canonicalize(&plan.target).map_err(|e| e.to_string())?
        {
            return Err("The update process does not match the target executable.".into());
        }
        if unsafe { WaitForSingleObject(process, 45000) } != WAIT_OBJECT_0 {
            return Err("The application did not exit in time. Nothing was replaced.".into());
        }
        Ok(())
    })();
    unsafe {
        CloseHandle(process);
    }
    result
}

pub fn apply_update() -> Result<(), String> {
    let helper = std::env::current_exe().map_err(|e| e.to_string())?;
    let directory = helper.parent().ok_or("Update staging folder missing.")?;
    let plan: Plan = serde_json::from_slice(
        &fs::read(directory.join("update.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let parent = plan.target.parent().ok_or("Invalid update target.")?;
    validate_stage(directory, parent)?;
    if plan
        .target
        .extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case("exe"))
        || plan.version != crate::VERSION
        || (plan.smoke_marker.is_some() && !cfg!(feature = "updater-smoke-test"))
    {
        return Err("The update plan does not match this executable.".into());
    }
    if hash(&helper)? != plan.new_sha {
        return Err("The update helper failed checksum verification.".into());
    }
    wait_for_parent(&plan)?;
    if hash(&plan.target)? != plan.original_sha {
        return Err(
            "The installed executable changed during the download. It was not overwritten.".into(),
        );
    }
    let ready = directory.join("ready.exe");
    let backup = directory.join("previous.exe");
    fs::copy(&helper, &ready).map_err(|e| e.to_string())?;
    fs::OpenOptions::new()
        .write(true)
        .open(&ready)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    if let Err(error) = replace(&plan.target, &ready, &backup) {
        if !plan.target.exists() && backup.exists() {
            let _ = fs::rename(&backup, &plan.target);
        }
        return Err(error);
    }
    let mut command = Command::new(&plan.target);
    if let Some(marker) = &plan.smoke_marker {
        command.arg("--updater-smoke-new").arg(marker);
    } else {
        command
            .arg("--settings")
            .arg("--updated")
            .arg("--cleanup-update")
            .arg(directory);
        if plan.portable {
            command.arg("--portable");
        }
    }
    command.creation_flags(CREATE_NO_WINDOW);
    match command.spawn() {
        Ok(mut child) => {
            if plan.smoke_marker.is_some() && !child.wait().map_err(|e| e.to_string())?.success() {
                return Err("Updated test child failed.".into());
            }
            Ok(())
        }
        Err(error) => {
            let restore = directory.join("restore.exe");
            fs::copy(&backup, &restore).map_err(|e| e.to_string())?;
            replace(&plan.target, &restore, &directory.join("failed.exe"))?;
            let mut previous = Command::new(&plan.target);
            previous.arg("--startup");
            if plan.portable {
                previous.arg("--portable");
            }
            let _ = previous.creation_flags(CREATE_NO_WINDOW).spawn();
            Err(format!(
                "The update could not restart ({error}). The previous version was restored."
            ))
        }
    }
}

fn validate_stage(directory: &Path, parent: &Path) -> Result<(), String> {
    let directory = fs::canonicalize(directory).map_err(|e| e.to_string())?;
    if !directory
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with(STAGE_PREFIX))
        || directory.parent()
            != Some(
                fs::canonicalize(parent)
                    .map_err(|e| e.to_string())?
                    .as_path(),
            )
    {
        return Err("The update staging folder is outside the executable's folder.".into());
    }
    Ok(())
}

fn cleanup(directory: &Path, parent: &Path) {
    if validate_stage(directory, parent).is_err() {
        return;
    }
    // Only known updater files are removed. Never recursively delete a supplied directory.
    for name in [
        STAGED_EXE,
        "ready.exe",
        "previous.exe",
        "restore.exe",
        "failed.exe",
        "update.json",
    ] {
        let _ = fs::remove_file(directory.join(name));
    }
    let _ = fs::remove_dir(directory);
}

pub fn cleanup_after_update(directory: PathBuf) {
    let Ok(target) = std::env::current_exe() else {
        return;
    };
    let Some(parent) = target.parent().map(Path::to_path_buf) else {
        return;
    };
    if validate_stage(&directory, &parent).is_err() {
        return;
    }
    let _ = thread::Builder::new()
        .name("update-cleanup".into())
        .spawn(move || {
            for _ in 0..50 {
                cleanup(&directory, &parent);
                if !directory.exists() {
                    break;
                }
                thread::sleep(Duration::from_millis(200));
            }
        });
}

#[cfg(feature = "updater-smoke-test")]
pub fn smoke_cli(arguments: &[std::ffi::OsString]) -> Result<bool, String> {
    if arguments.len() == 1 && arguments[0] == "--updater-smoke-old" {
        thread::sleep(Duration::from_millis(3000));
        return Ok(true);
    }
    if arguments.len() == 2 && arguments[0] == "--updater-smoke-new" {
        let marker = PathBuf::from(&arguments[1]);
        if marker.file_name().is_none_or(|n| n != "restarted.txt")
            || marker
                .parent()
                .and_then(Path::file_name)
                .is_none_or(|n| !n.to_string_lossy().starts_with("updater-smoke-"))
        {
            return Err("Invalid updater test marker.".into());
        }
        fs::write(marker, b"restarted").map_err(|e| e.to_string())?;
        return Ok(true);
    }
    if arguments.len() != 1 || arguments[0] != "--test-updater" {
        return Ok(false);
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("target")
        .join(format!("updater-smoke-{}-{}", std::process::id(), now()));
    fs::create_dir(&parent).map_err(|e| e.to_string())?;
    let target = parent.join(STAGED_EXE);
    fs::copy(&executable, &target).map_err(|e| e.to_string())?;
    fs::OpenOptions::new()
        .append(true)
        .open(&target)
        .and_then(|mut f| f.write_all(b"old-version-fixture"))
        .map_err(|e| e.to_string())?;
    let directory = new_stage(&parent)?;
    fs::copy(&executable, directory.join(STAGED_EXE)).map_err(|e| e.to_string())?;
    let marker = parent.join("restarted.txt");
    let original_sha = hash(&target)?;
    let new_sha = hash(&executable)?;
    if original_sha == new_sha {
        return Err("Update test fixture must differ.".into());
    }
    let mut old = Command::new(&target)
        .arg("--updater-smoke-old")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    let plan = Plan {
        target: target.clone(),
        parent_pid: old.id(),
        original_sha: original_sha.clone(),
        new_sha: new_sha.clone(),
        version: crate::VERSION.into(),
        portable: true,
        smoke_marker: Some(marker.clone()),
    };
    fs::write(
        directory.join("update.json"),
        serde_json::to_vec(&plan).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let status = Command::new(directory.join(STAGED_EXE))
        .arg("--apply-update")
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    let _ = old.wait();
    if !status.success()
        || hash(&target)? != new_sha
        || hash(&directory.join("previous.exe"))? != original_sha
        || fs::read(&marker).map_err(|e| e.to_string())? != b"restarted"
    {
        return Err("Native update replacement/restart regression failed.".into());
    }
    cleanup(&directory, &parent);
    fs::remove_file(&target).map_err(|e| e.to_string())?;
    fs::remove_file(marker).map_err(|e| e.to_string())?;
    fs::remove_dir(parent).map_err(|e| e.to_string())?;
    println!(
        "PASS wait for running executable, verified replacement, old-version backup, restart, and isolated cleanup"
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release_json(tag: &str) -> serde_json::Value {
        let executable = format!("MultiMonitorBrightness-{tag}.exe");
        serde_json::json!({"tag_name":tag,"html_url":format!("{REPO}/releases/tag/{tag}"),"draft":false,"prerelease":false,
            "assets":[{"name":executable,"browser_download_url":format!("{REPO}/releases/download/{tag}/{executable}"),"size":4096},
            {"name":"SHA256SUMS.txt","browser_download_url":format!("{REPO}/releases/download/{tag}/SHA256SUMS.txt"),"size":93}]})
    }
    #[test]
    fn stable_release_selection_is_semantic_and_repo_bound() {
        assert!(
            parse_release(&release_json("v0.10.0").to_string(), "0.9.0")
                .unwrap()
                .is_some()
        );
        assert!(
            parse_release(&release_json("v0.1.2").to_string(), "0.2.0")
                .unwrap()
                .is_none()
        );
        let mut json = release_json("v9.0.0");
        json["assets"][0]["browser_download_url"] = "https://example.com/tool.exe".into();
        assert!(parse_release(&json.to_string(), "0.2.0").is_err());
        json = release_json("v9.0.0");
        json["assets"][0]["name"] = "MultiMonitorBrightness-v8.0.0.exe".into();
        assert!(parse_release(&json.to_string(), "0.2.0").is_err());
        json = release_json("v9.0.0");
        json["assets"][0]["name"] = STAGED_EXE.into();
        assert!(parse_release(&json.to_string(), "0.2.0").is_err());
        json = release_json("v9.0.0");
        json["prerelease"] = true.into();
        assert!(parse_release(&json.to_string(), "0.2.0").unwrap().is_none());
    }
    #[test]
    fn checksum_is_bound_to_the_versioned_asset_and_rejects_ambiguous_entries() {
        let hash = "ab".repeat(32);
        let name = "MultiMonitorBrightness-v0.2.1.exe";
        assert_eq!(checksum(&format!("{hash}  {name}\n"), name).unwrap(), hash);
        assert!(checksum("no checksum", name).is_err());
        assert!(checksum(&format!("{hash} {name}\n{hash} {name}"), name).is_err());
        assert!(checksum(&format!("{} {name}", "gg".repeat(32)), name).is_err());
        assert!(checksum(&format!("{hash} MultiMonitorBrightness-v0.2.0.exe"), name).is_err());
        assert!(checksum(&format!("{hash} {STAGED_EXE}"), name).is_err());
    }
    #[test]
    fn replacement_keeps_a_recoverable_backup_and_does_not_touch_preferences() {
        let parent = std::env::current_dir().unwrap().join("target");
        let directory = new_stage(&parent).unwrap();
        let target = directory.join("installed.exe");
        let ready = directory.join("next.exe");
        let backup = directory.join("backup.exe");
        fs::write(&target, b"old binary").unwrap();
        fs::write(&ready, b"new binary").unwrap();
        fs::write(directory.join("settings.json"), b"user preferences").unwrap();
        replace(&target, &ready, &backup).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new binary");
        assert_eq!(fs::read(&backup).unwrap(), b"old binary");
        assert_eq!(
            fs::read(directory.join("settings.json")).unwrap(),
            b"user preferences"
        );
        for name in ["installed.exe", "backup.exe", "settings.json"] {
            fs::remove_file(directory.join(name)).unwrap();
        }
        fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn executable_validation_rejects_corruption_truncation_and_wrong_architecture() {
        let directory = new_stage(&std::env::current_dir().unwrap().join("target")).unwrap();
        let path = directory.join(STAGED_EXE);
        let mut bytes = vec![0u8; 1024];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..70].copy_from_slice(b"PE\0\0\x64\x86");
        fs::write(&path, &bytes).unwrap();
        let expected = hash(&path).unwrap();
        assert!(verify_executable(&path, &expected, 1024).is_ok());
        bytes[900] = 1;
        fs::write(&path, &bytes).unwrap();
        assert!(verify_executable(&path, &expected, 1024).is_err());
        assert!(verify_executable(&path, &hash(&path).unwrap(), 2048).is_err());
        bytes[68..70].copy_from_slice(&0x14cu16.to_le_bytes());
        fs::write(&path, &bytes).unwrap();
        assert!(verify_executable(&path, &hash(&path).unwrap(), 1024).is_err());
        bytes[60..64].copy_from_slice(&2048u32.to_le_bytes());
        fs::write(&path, &bytes).unwrap();
        assert!(verify_executable(&path, &hash(&path).unwrap(), 1024).is_err());
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn cleanup_rejects_non_staging_paths_and_preserves_unknown_files() {
        let parent = std::env::current_dir().unwrap().join("target");
        assert!(validate_stage(&parent, &parent).is_err());
        let directory = new_stage(&parent).unwrap();
        assert!(validate_stage(&directory, &parent.join("debug")).is_err());
        fs::write(directory.join(STAGED_EXE), b"temporary executable").unwrap();
        fs::write(directory.join("keep.txt"), b"unrelated data").unwrap();
        cleanup(&directory, &parent);
        assert!(!directory.join(STAGED_EXE).exists());
        assert_eq!(
            fs::read(directory.join("keep.txt")).unwrap(),
            b"unrelated data"
        );
        fs::remove_file(directory.join("keep.txt")).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    #[ignore = "Downloads and verifies a public GitHub release; never executes or installs it"]
    fn public_release_download_verifies() {
        let release = check_latest("0.0.0").unwrap().expect("published release");
        eprintln!(
            "Verifying GitHub release {}: {}",
            release.version, release.executable.name
        );
        let prepared = prepare(&release, false, |_, _| {}).unwrap();
        let exe = std::env::current_exe().unwrap();
        cleanup(&prepared.directory, exe.parent().unwrap());
    }
}
