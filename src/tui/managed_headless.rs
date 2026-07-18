//! Safe, local ownership for the `chromewright --headless tui` browser.
//!
//! [`ManagedHeadlessSession`] launches or reuses a private headless Chrome under
//! a narrow ownership lease (PID, nonce, private profile, DevTools port). Before
//! reconnecting, stopping, or accepting a DevTools endpoint we prove those
//! fields still belong together. A lease is never authority to signal an
//! unrelated Chrome process. [`BrowserSessionPolicy`] chooses reuse vs restart.

use crate::{BrowserSession, ConnectionOptions, LaunchOptions};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LEASE_NAME: &str = "managed-headless.json";
const LOCK_NAME: &str = ".managed-headless.lock";
const OWNER_NAME: &str = ".chromewright-owner";
const STARTUP_ATTEMPTS: usize = 3;
const INITIAL_PAGE_URL: &str = "about:blank";

/// How `--headless tui` handles a previous Chromewright-owned browser.
///
/// Applies only to managed headless leases; external `--ws-endpoint` attach
/// sessions never use this policy and are never terminated by the TUI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum BrowserSessionPolicy {
    /// Reconnect to a healthy owned browser, otherwise launch a new one.
    #[default]
    Reuse,
    /// Replace a healthy owned browser before starting the TUI.
    Restart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LeaseRecord {
    version: u8,
    nonce: String,
    pid: u32,
    port: u16,
    profile: PathBuf,
}

/// A connected session and the lease responsible for safe shutdown.
pub struct ManagedHeadlessSession {
    // The TUI must receive the only `Arc<BrowserSession>` so it can register
    // its tools before sharing that session with the companion server. Keep
    // the plain session here until `take_session` transfers ownership; this
    // manager otherwise only owns the lease, child, and shutdown policy.
    session: Option<BrowserSession>,
    root: PathBuf,
    lease: LeaseRecord,
    child: Option<Child>,
    // Kept open for the entire TUI lifetime. The OS releases this advisory
    // lock if the process crashes, unlike a create_new sentinel file.
    _lock: RootLock,
}

impl ManagedHeadlessSession {
    /// Create or recover the managed headless browser used only by the TUI.
    pub fn open(launch: &LaunchOptions, policy: BrowserSessionPolicy) -> Result<Self, String> {
        if launch.debug_port == Some(0) {
            return Err("--debug-port must be between 1 and 65535 for --headless tui".into());
        }
        let root = runtime_root()?;
        let mut lock = RootLock::acquire(&root)?;

        if let Some(existing) = read_lease(&root)? {
            let owned = owned_and_live(&root, &existing);
            // A pinned port is an explicit caller contract. Reuse is only
            // permitted when the owned lease uses that same port; otherwise
            // safely replace the verified old child before launching there.
            let port_compatible = port_is_compatible(launch.debug_port, existing.port);
            if owned && port_compatible && policy == BrowserSessionPolicy::Reuse {
                // A TCP/CDP endpoint is accepted only after the lease has proven
                // that the matching browser process still owns this session.
                if listener_belongs_to(&existing)
                    && let Ok(session) =
                        BrowserSession::connect(ConnectionOptions::new(endpoint(existing.port)))
                    && listener_belongs_to(&existing)
                    && let Ok(session) = ensure_managed_page_target(session)
                {
                    return Ok(Self {
                        session: Some(session),
                        root,
                        lease: existing,
                        child: None,
                        _lock: lock,
                    });
                }
            }
            if owned {
                terminate_owned(&existing)?;
                cleanup_owned(&root, &existing)?;
            } else {
                // A stale or unverifiable record never authorizes a signal,
                // but it still retains its lease until safe private-profile
                // cleanup succeeds.
                cleanup_owned(&root, &existing)?;
            }
        }

        let attempts = if launch.debug_port.is_some() {
            1
        } else {
            STARTUP_ATTEMPTS
        };
        let mut last_error = None;
        for _ in 0..attempts {
            let port = launch.debug_port.unwrap_or(available_loopback_port()?);
            match launch_new(&root, launch, port, lock) {
                Ok(session) => return Ok(session),
                Err(error) => {
                    last_error = Some(error);
                    // A failed launch consumes its lifetime lock with the
                    // failed session attempt. Reacquire for the next try.
                    lock = RootLock::acquire(&root)?;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| "unable to start managed headless Chrome".into()))
    }

    /// Transfer the browser session to the TUI without creating a second
    /// strong reference. The manager remains alive to retain its runtime lock
    /// and safely terminate the owned Chrome process afterwards.
    pub fn take_session(&mut self) -> Result<std::sync::Arc<BrowserSession>, String> {
        self.session
            .take()
            .map(std::sync::Arc::new)
            .ok_or_else(|| "managed headless browser session was already transferred".into())
    }

    /// Shut down only the browser whose ownership can still be proven.
    pub fn shutdown(mut self) -> Result<(), String> {
        drop(self.session);
        let stopped = if let Some(child) = self.child.take() {
            stop_child(child, &self.root, &self.lease)
        } else if owned_and_live(&self.root, &self.lease) {
            terminate_owned(&self.lease)
        } else {
            Err("managed headless Chrome ownership verification failed during shutdown".into())
        };
        // Do not remove the lease or private profile if termination cannot be
        // proven. A later explicit recovery can inspect the intact evidence.
        stopped?;
        cleanup_owned(&self.root, &self.lease)
    }
}

#[derive(Debug)]
struct RootLock {
    _file: fs::File,
}
impl RootLock {
    fn acquire(root: &Path) -> Result<Self, String> {
        let path = root.join(LOCK_NAME);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| format!("open managed Chrome session lock: {error}"))?;
        match file.try_lock_exclusive() {
            Ok(()) => {
                file.set_len(0)
                    .map_err(|error| format!("reset managed Chrome session lock: {error}"))?;
                writeln!(file, "{}", std::process::id())
                    .map_err(|error| format!("write managed Chrome session lock: {error}"))?;
                Ok(Self { _file: file })
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                Err("managed headless Chrome session is already active in another TUI".into())
            }
            Err(error) => Err(format!("lock managed Chrome runtime: {error}")),
        }
    }
}

fn launch_new(
    root: &Path,
    options: &LaunchOptions,
    port: u16,
    lock: RootLock,
) -> Result<ManagedHeadlessSession, String> {
    let nonce = nonce();
    let profile = root.join(format!("profile-{nonce}"));
    fs::create_dir(&profile).map_err(|e| format!("create managed Chrome profile: {e}"))?;
    if let Err(error) = write_private(&profile.join(OWNER_NAME), nonce.as_bytes()) {
        let _ = remove_new_profile(root, &profile, &nonce);
        return Err(error);
    }

    let executable = match chrome_executable(options) {
        Ok(executable) => executable,
        Err(error) => {
            let _ = remove_profile_if_safe(root, &profile, &nonce);
            return Err(error);
        }
    };
    let mut command = Command::new(executable);
    command
        .arg("--headless=new")
        .arg("--remote-debugging-address=127.0.0.1")
        .arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--chromewright-managed-session={nonce}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-background-networking")
        .arg("--disable-dev-shm-usage")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if !options.sandbox {
        command.arg("--no-sandbox");
    }
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = remove_profile_if_safe(root, &profile, &nonce);
            return Err(format!("launch managed headless Chrome: {error}"));
        }
    };
    let lease = LeaseRecord {
        version: 1,
        nonce,
        pid: child.id(),
        port,
        profile,
    };
    if let Err(error) = write_lease(root, &lease) {
        // We still own this direct child even though publishing its lease
        // failed.  Do not delete its profile unless waiting on this exact
        // child proves it has exited; otherwise retain the evidence for
        // manual recovery rather than risking an in-use profile.
        return match stop_spawned_child(child) {
            Ok(()) => match remove_profile_if_safe(root, &lease.profile, &lease.nonce) {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(format!(
                    "{error}; managed Chrome stopped but its private profile was retained: {cleanup_error}"
                )),
            },
            Err(stop_error) => Err(format!(
                "{error}; failed to confirm managed Chrome stopped, so its private profile was retained: {stop_error}"
            )),
        };
    }

    let mut child = child;
    for _ in 0..40 {
        // Never trust an occupied pinned port or endpoint unless the child we
        // just launched is still the process described by this exact lease.
        if child.try_wait().map_err(|e| e.to_string())?.is_none()
            && listener_belongs_to(&lease)
            && let Ok(session) = BrowserSession::connect(ConnectionOptions::new(endpoint(port)))
            && child.try_wait().map_err(|e| e.to_string())?.is_none()
            && listener_belongs_to(&lease)
            && let Ok(session) = ensure_managed_page_target(session)
        {
            return Ok(ManagedHeadlessSession {
                session: Some(session),
                root: root.to_path_buf(),
                lease,
                child: Some(child),
                _lock: lock,
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
    let startup_error = format!(
        "managed headless Chrome did not become ready at {}",
        endpoint(port)
    );
    match stop_child(child, root, &lease) {
        Ok(()) => match cleanup_owned(root, &lease) {
            Ok(()) => Err(startup_error),
            Err(cleanup_error) => Err(format!(
                "{startup_error}; managed Chrome stopped but its lease and private profile were retained: {cleanup_error}"
            )),
        },
        Err(stop_error) => Err(format!(
            "{startup_error}; failed to confirm managed Chrome stopped, so its lease and private profile were retained: {stop_error}"
        )),
    }
}

/// Headless Chrome does not expose a visible/focused target, so an attach-style
/// connection to a reusable page can have no active page. This applies only
/// to the private browser we launched and verified above: first reacquire an
/// existing page target from the inventory, creating a fresh blank page only
/// when the inventory is empty.
///
/// External `--ws-endpoint` sessions continue to use the normal attach policy;
/// this helper is intentionally called only from managed-headless startup and
/// recovery after the lease and listener PID have been proven.
fn ensure_managed_page_target(session: BrowserSession) -> Result<BrowserSession, String> {
    let tabs = session
        .list_tabs()
        .map_err(|error| format!("inspect managed headless tabs: {error}"))?;
    if tabs.iter().any(|tab| tab.active) {
        return Ok(session);
    }

    if let Some(tab) = tabs.first() {
        session
            .activate_tab(&tab.id)
            .map_err(|error| format!("reacquire managed headless page '{}': {error}", tab.id))?;
    } else {
        session
            .open_tab(INITIAL_PAGE_URL)
            .map_err(|error| format!("create managed headless initial page: {error}"))?;
    }
    let has_active_tab = session
        .list_tabs()
        .map_err(|error| format!("verify managed headless page target: {error}"))?
        .iter()
        .any(|tab| tab.active);
    if !has_active_tab {
        return Err("managed headless Chrome did not expose an active page target".into());
    }

    Ok(session)
}

fn runtime_root() -> Result<PathBuf, String> {
    let root = if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime)
            .join("chromewright")
            .join("managed-headless")
    } else if let Some(home) = std::env::var_os("HOME") {
        #[cfg(target_os = "macos")]
        let cache = PathBuf::from(home).join("Library").join("Caches");
        #[cfg(not(target_os = "macos"))]
        let cache = PathBuf::from(home).join(".cache");
        cache.join("chromewright").join("managed-headless")
    } else {
        return Err(
            "cannot locate a private runtime directory; set XDG_RUNTIME_DIR or HOME".into(),
        );
    };
    fs::create_dir_all(&root)
        .map_err(|e| format!("create managed Chrome runtime directory: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("secure managed Chrome runtime directory: {e}"))?;
    }
    root.canonicalize()
        .map_err(|e| format!("canonicalize managed Chrome runtime directory: {e}"))
}

fn available_loopback_port() -> Result<u16, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("allocate managed Chrome DevTools port: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    Ok(port)
}
fn endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}
fn port_is_compatible(requested: Option<u16>, leased: u16) -> bool {
    requested.is_none_or(|port| port == leased)
}
fn nonce() -> String {
    format!(
        "{:x}-{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}
fn chrome_executable(options: &LaunchOptions) -> Result<PathBuf, String> {
    if let Some(path) = &options.chrome_path {
        return Ok(path.clone());
    }
    if let Some(path) = std::env::var_os("CHROME") {
        return Ok(PathBuf::from(path));
    }
    #[cfg(target_os = "macos")]
    {
        Ok(PathBuf::from(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(PathBuf::from("chrome.exe"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Ok(PathBuf::from("google-chrome"))
    }
}

fn lease_path(root: &Path) -> PathBuf {
    root.join(LEASE_NAME)
}
fn read_lease(root: &Path) -> Result<Option<LeaseRecord>, String> {
    match fs::read(lease_path(root)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("read managed Chrome lease: {e}")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read managed Chrome lease: {e}")),
    }
}
fn write_lease(root: &Path, lease: &LeaseRecord) -> Result<(), String> {
    let path = lease_path(root);
    let temp = root.join(format!(".{LEASE_NAME}.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec(lease).map_err(|e| e.to_string())?;
    write_private(&temp, &bytes)?;
    fs::rename(&temp, &path).map_err(|e| format!("publish managed Chrome lease: {e}"))
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::File::create(path).map_err(|e| format!("write {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    file.sync_all()
        .map_err(|e| format!("sync {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("secure {}: {e}", path.display()))?;
    }
    Ok(())
}
fn remove_lease_if_nonce(root: &Path, nonce: &str) -> Result<(), String> {
    let Some(current) = read_lease(root)? else {
        return Ok(());
    };
    if current.nonce != nonce {
        return Ok(());
    }
    match fs::remove_file(lease_path(root)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove managed Chrome lease: {e}")),
    }
}

fn safe_profile(root: &Path, profile: &Path, nonce: &str) -> bool {
    let expected = root.join(format!("profile-{nonce}"));
    if profile != expected {
        return false;
    }
    let Ok(metadata) = fs::symlink_metadata(profile) else {
        return false;
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return false;
    }
    let Ok(canonical) = profile.canonicalize() else {
        return false;
    };
    if canonical != expected {
        return false;
    }
    let marker = profile.join(OWNER_NAME);
    let Ok(marker_meta) = fs::symlink_metadata(&marker) else {
        return false;
    };
    !marker_meta.file_type().is_symlink()
        && marker_meta.is_file()
        && fs::read_to_string(marker).ok().as_deref() == Some(nonce)
}
fn remove_profile_if_safe(root: &Path, profile: &Path, nonce: &str) -> Result<(), String> {
    if !safe_profile(root, profile, nonce) {
        return Err("managed Chrome profile ownership verification failed".into());
    }
    fs::remove_dir_all(profile).map_err(|e| format!("remove managed Chrome profile: {e}"))
}
/// Cleanup for the small window after creating the nonce-named directory but
/// before its ownership marker can be written. The exact direct-child and
/// no-symlink checks keep this distinct from lease-based cleanup.
fn remove_new_profile(root: &Path, profile: &Path, nonce: &str) -> Result<(), String> {
    let expected = root.join(format!("profile-{nonce}"));
    if profile != expected {
        return Err("managed Chrome profile path verification failed".into());
    }
    let metadata = fs::symlink_metadata(profile)
        .map_err(|e| format!("inspect managed Chrome profile: {e}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || profile.canonicalize().ok().as_deref() != Some(expected.as_path())
    {
        return Err("managed Chrome profile path verification failed".into());
    }
    fs::remove_dir_all(profile).map_err(|e| format!("remove managed Chrome profile: {e}"))
}
fn cleanup_owned(root: &Path, lease: &LeaseRecord) -> Result<(), String> {
    // Keep the nonce-matched lease as recovery evidence until private-profile
    // ownership is re-proven and removal succeeds.
    remove_profile_if_safe(root, &lease.profile, &lease.nonce)?;
    remove_lease_if_nonce(root, &lease.nonce)
}
fn owned_and_live(root: &Path, lease: &LeaseRecord) -> bool {
    lease.version == 1
        && lease.port != 0
        && safe_profile(root, &lease.profile, &lease.nonce)
        && process_matches(lease)
}

/// Fail closed unless `lsof` proves that exactly this managed Chrome PID owns
/// the loopback listener. A valid lease and command line are insufficient:
/// another process can occupy the port between validation and attach.
#[cfg(unix)]
fn listener_belongs_to(lease: &LeaseRecord) -> bool {
    let output = Command::new("lsof")
        .args([
            "-nP",
            "-a",
            &format!("-iTCP:{}", lease.port),
            "-sTCP:LISTEN",
            "-Fp",
        ])
        .output();
    let Ok(output) = output else {
        return false;
    };
    output.status.success() && listener_pid_from_lsof(&output.stdout) == Some(lease.pid)
}
#[cfg(not(unix))]
fn listener_belongs_to(_: &LeaseRecord) -> bool {
    false
}

fn listener_pid_from_lsof(output: &[u8]) -> Option<u32> {
    let pids = String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| line.strip_prefix('p'))
        .filter_map(|pid| pid.parse::<u32>().ok())
        .collect::<BTreeSet<_>>();
    (pids.len() == 1).then(|| *pids.first().expect("one listener PID"))
}

#[cfg(unix)]
fn process_matches(lease: &LeaseRecord) -> bool {
    let output = Command::new("ps")
        .args(["-p", &lease.pid.to_string(), "-o", "command="])
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let command = String::from_utf8_lossy(&output.stdout);
    command.contains(&format!("--remote-debugging-port={}", lease.port))
        && command.contains(&format!("--user-data-dir={}", lease.profile.display()))
        && command.contains(&format!("--chromewright-managed-session={}", lease.nonce))
}
#[cfg(not(unix))]
fn process_matches(_: &LeaseRecord) -> bool {
    false
}

#[cfg(unix)]
fn terminate_owned(lease: &LeaseRecord) -> Result<(), String> {
    if !process_matches(lease) {
        return Err("managed headless Chrome ownership verification failed".into());
    }
    let term = Command::new("kill")
        .args(["-TERM", &lease.pid.to_string()])
        .status()
        .map_err(|e| format!("stop managed headless Chrome: {e}"))?;
    if !term.success() {
        return Err("stop managed headless Chrome: process rejected SIGTERM".into());
    }
    for _ in 0..20 {
        if !process_matches(lease) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    let kill = Command::new("kill")
        .args(["-KILL", &lease.pid.to_string()])
        .status()
        .map_err(|e| format!("kill managed headless Chrome: {e}"))?;
    if !kill.success() {
        return Err("kill managed headless Chrome: process rejected SIGKILL".into());
    }
    for _ in 0..20 {
        if !process_matches(lease) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("managed headless Chrome did not exit after SIGKILL".into())
}
#[cfg(not(unix))]
fn terminate_owned(_: &LeaseRecord) -> Result<(), String> {
    Err("managed headless Chrome termination is unsupported on this platform".into())
}

fn stop_child(mut child: Child, root: &Path, lease: &LeaseRecord) -> Result<(), String> {
    // `Child` is stronger evidence than the lease: it names the exact process
    // launched for this attempt even after that process has exited and is no
    // longer observable through `ps`.  In that common startup-failure case,
    // requiring `owned_and_live` would retain the matching lease/profile and
    // let the next retry overwrite the lease, orphaning the private profile.
    if child
        .try_wait()
        .map_err(|e| format!("inspect managed Chrome child: {e}"))?
        .is_some()
    {
        return Ok(());
    }
    if !owned_and_live(root, lease) {
        return Err("managed headless Chrome ownership verification failed".into());
    }
    if let Err(error) = terminate_owned(lease) {
        // The child can exit between the liveness check and the signal. Reap
        // that exact child before treating the ownership proof failure as a
        // retained-evidence condition.
        if child
            .try_wait()
            .map_err(|wait_error| format!("inspect managed Chrome child: {wait_error}"))?
            .is_some()
        {
            return Ok(());
        }
        return Err(error);
    }
    child
        .wait()
        .map_err(|e| format!("wait for managed headless Chrome: {e}"))?;
    Ok(())
}

/// Stop a child we spawned but could not publish a lease for.  A `Child`
/// handle identifies the exact process, so this remains safe before the
/// browser is listening or its command line is observable.
fn stop_spawned_child(mut child: Child) -> Result<(), String> {
    if child
        .try_wait()
        .map_err(|error| format!("inspect managed Chrome child: {error}"))?
        .is_none()
    {
        child
            .kill()
            .map_err(|error| format!("stop unpublished managed Chrome child: {error}"))?;
    }
    child
        .wait()
        .map_err(|error| format!("wait for unpublished managed Chrome child: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_transfer_gives_tui_the_only_arc() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let mut managed = ManagedHeadlessSession {
            session: Some(BrowserSession::with_test_backend(
                crate::browser::backend::FakeSessionBackend::new(),
            )),
            root: root.clone(),
            lease: LeaseRecord {
                version: 1,
                nonce: "transfer-test".into(),
                pid: 1,
                port: 1,
                profile: root.join("profile-transfer-test"),
            },
            child: None,
            _lock: RootLock::acquire(&root).unwrap(),
        };

        let mut session = managed.take_session().expect("transfer session to TUI");
        assert!(
            std::sync::Arc::get_mut(&mut session).is_some(),
            "the TUI must be able to register its tools before sharing the session"
        );
        assert!(managed.take_session().is_err());
    }

    #[test]
    fn managed_bootstrap_reacquires_existing_blank_page_without_creating_one() {
        let session = BrowserSession::with_test_backend(
            crate::browser::backend::FakeSessionBackend::with_no_active_tab(),
        );

        let session = ensure_managed_page_target(session)
            .expect("managed headless bootstrap should reacquire the blank page");
        let tabs = session.list_tabs().expect("list tabs after bootstrap");

        assert_eq!(tabs.len(), 1, "the existing tab is reacquired in place");
        assert!(
            tabs.iter()
                .any(|tab| { tab.active && tab.url == INITIAL_PAGE_URL && tab.id == "tab-1" })
        );
        assert!(session.document_metadata().is_ok());
    }

    #[test]
    fn managed_bootstrap_creates_an_active_blank_page_for_empty_inventory() {
        let session = BrowserSession::with_test_backend(
            crate::browser::backend::FakeSessionBackend::with_no_tabs(),
        );

        let session = ensure_managed_page_target(session)
            .expect("managed headless bootstrap should create an initial page");
        let tabs = session.list_tabs().expect("list tabs after bootstrap");

        assert_eq!(tabs.len(), 1, "an empty browser gets one initial tab");
        assert!(tabs[0].active);
        assert_eq!(tabs[0].url, INITIAL_PAGE_URL);
        assert!(session.document_metadata().is_ok());
    }

    #[test]
    fn managed_bootstrap_reacquires_existing_page_without_creating_blank_tab() {
        let existing_url = "https://example.com/continuity";
        let session = BrowserSession::with_test_backend(
            crate::browser::backend::FakeSessionBackend::with_nonblank_tab_without_active(
                existing_url,
            ),
        );

        let session = ensure_managed_page_target(session)
            .expect("managed headless reuse should reacquire its existing page");
        let tabs = session.list_tabs().expect("list tabs after reuse");

        assert_eq!(tabs.len(), 1, "reuse must not create a redundant blank tab");
        assert_eq!(tabs[0].url, existing_url);
        assert!(
            tabs[0].active,
            "the retained page becomes the active target"
        );
        assert!(
            session.document_metadata().is_ok(),
            "retained page metadata is usable"
        );
    }

    #[test]
    fn managed_bootstrap_keeps_existing_active_page() {
        let session =
            BrowserSession::with_test_backend(crate::browser::backend::FakeSessionBackend::new());

        let session = ensure_managed_page_target(session)
            .expect("an existing active page should remain usable");
        let tabs = session.list_tabs().expect("list tabs after bootstrap");

        assert_eq!(tabs.len(), 1, "no redundant bootstrap tab is created");
        assert!(tabs[0].active);
    }

    #[test]
    fn nonce_and_free_port_are_usable() {
        assert_ne!(nonce(), nonce());
        match available_loopback_port() {
            Ok(port) => assert_ne!(port, 0),
            Err(error) if error.contains("Operation not permitted") => {
                eprintln!("skipping loopback allocation assertion: sandbox denies binds")
            }
            Err(error) => panic!("allocate loopback port: {error}"),
        }
    }
    #[test]
    fn zero_port_is_rejected_before_runtime_setup() {
        let options = LaunchOptions {
            debug_port: Some(0),
            ..Default::default()
        };
        let result = ManagedHeadlessSession::open(&options, BrowserSessionPolicy::Reuse);
        assert!(matches!(result, Err(ref error) if error.contains("--debug-port")));
    }
    #[test]
    fn unsafe_or_indirect_profile_never_proves_ownership() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let lease = LeaseRecord {
            version: 1,
            nonce: "test".into(),
            pid: std::process::id(),
            port: 9222,
            profile: root.join("not-profile-test"),
        };
        assert!(!owned_and_live(&root, &lease));
    }
    #[test]
    fn nonce_matched_removal_preserves_replaced_lease() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let first = LeaseRecord {
            version: 1,
            nonce: "first".into(),
            pid: 1,
            port: 1,
            profile: root.join("profile-first"),
        };
        let second = LeaseRecord {
            nonce: "second".into(),
            ..first.clone()
        };
        write_lease(root, &second).unwrap();
        remove_lease_if_nonce(root, "first").unwrap();
        assert_eq!(read_lease(root).unwrap().unwrap().nonce, "second");
    }

    #[test]
    fn cleanup_retains_lease_when_profile_removal_is_not_safe() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let lease = LeaseRecord {
            version: 1,
            nonce: "retained".into(),
            pid: 1,
            port: 1,
            // No profile or ownership marker is created, so cleanup must
            // fail without deleting the lease needed for recovery.
            profile: root.join("profile-retained"),
        };
        write_lease(root, &lease).unwrap();

        assert!(cleanup_owned(root, &lease).is_err());
        assert_eq!(read_lease(root).unwrap().unwrap().nonce, lease.nonce);
    }

    #[cfg(unix)]
    #[test]
    fn exited_child_startup_failure_releases_its_matching_lease_and_profile() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let nonce = "exited-startup-child";
        let profile = root.join(format!("profile-{nonce}"));
        fs::create_dir(&profile).unwrap();
        write_private(&profile.join(OWNER_NAME), nonce.as_bytes()).unwrap();

        let mut child = Command::new("sh")
            .args(["-c", "exit 0"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.wait().unwrap();
        let lease = LeaseRecord {
            version: 1,
            nonce: nonce.into(),
            pid: child.id(),
            port: 9222,
            profile: profile.clone(),
        };
        write_lease(&root, &lease).unwrap();

        // This models a browser exiting before readiness. It must be accepted
        // through the exact Child handle, rather than requiring a live `ps`
        // match, so the retry can clean up before publishing another lease.
        stop_child(child, &root, &lease).unwrap();
        cleanup_owned(&root, &lease).unwrap();

        assert!(!profile.exists());
        assert!(read_lease(&root).unwrap().is_none());
    }

    #[test]
    fn pinned_port_never_reuses_a_different_lease() {
        assert!(port_is_compatible(None, 9222));
        assert!(port_is_compatible(Some(9222), 9222));
        assert!(!port_is_compatible(Some(9223), 9222));
    }

    #[test]
    fn listener_proof_requires_one_exact_pid() {
        assert_eq!(listener_pid_from_lsof(b"p42\n"), Some(42));
        assert_eq!(listener_pid_from_lsof(b"p42\np42\n"), Some(42));
        assert_eq!(listener_pid_from_lsof(b"p42\np43\n"), None);
        assert_eq!(listener_pid_from_lsof(b"garbage\n"), None);
    }

    #[cfg(unix)]
    #[test]
    fn listener_proof_rejects_a_real_foreign_loopback_listener() {
        let listener = match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("skipping loopback listener ownership test: sandbox denies binds");
                return;
            }
            Err(error) => panic!("bind loopback listener: {error}"),
        };
        let port = listener.local_addr().unwrap().port();
        let own_pid = std::process::id();
        let lease = LeaseRecord {
            version: 1,
            nonce: "listener-test".into(),
            pid: own_pid,
            port,
            profile: PathBuf::new(),
        };
        assert!(
            listener_belongs_to(&lease),
            "lsof must identify this listener"
        );

        let foreign = LeaseRecord {
            pid: own_pid
                .checked_add(1)
                .or_else(|| own_pid.checked_sub(1))
                .expect("a process id has a distinct neighbor"),
            ..lease
        };
        assert!(
            !listener_belongs_to(&foreign),
            "a listener owned by another PID must not prove lease ownership"
        );
        drop(listener);
    }

    #[test]
    fn lifetime_lock_rejects_a_second_tui_and_releases_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let first = RootLock::acquire(temp.path()).unwrap();
        let error = RootLock::acquire(temp.path()).unwrap_err();
        assert!(error.contains("already active"));
        drop(first);
        RootLock::acquire(temp.path()).expect("lock is OS-released with its owner");
    }

    #[cfg(unix)]
    #[test]
    fn termination_refuses_an_unverified_lease() {
        let temp = tempfile::tempdir().unwrap();
        let lease = LeaseRecord {
            version: 1,
            nonce: "not-this-process".into(),
            pid: std::process::id(),
            port: 65535,
            profile: temp.path().join("profile-not-this-process"),
        };
        assert!(matches!(
            terminate_owned(&lease),
            Err(error) if error.contains("ownership verification failed")
        ));
    }
}
