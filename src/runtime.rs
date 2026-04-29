use anyhow::{Context, Result};
use rand::{RngCore, rngs::OsRng};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const RUNTIME_ROOT_ENV: &str = "ALTAIR_VEGA_RUNTIME_ROOT";
pub const KEEP_RUNTIME_ENV: &str = "ALTAIR_VEGA_KEEP_RUNTIME";

pub struct DisposableRuntime {
    root: PathBuf,
    cleanup: bool,
}

impl DisposableRuntime {
    pub fn create(prefix: &str) -> Result<Self> {
        Self::create_in_with_cleanup(
            preferred_runtime_parent(),
            prefix,
            !keep_runtime_requested(),
        )
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    fn create_in_with_cleanup(
        parent: impl AsRef<Path>,
        prefix: &str,
        cleanup: bool,
    ) -> Result<Self> {
        let parent = parent.as_ref();
        fs::create_dir_all(parent)
            .with_context(|| format!("create runtime parent {}", parent.display()))?;

        let mut suffix_bytes = [0u8; 8];
        OsRng.fill_bytes(&mut suffix_bytes);
        let suffix = u64::from_be_bytes(suffix_bytes);
        let root = parent.join(format!("altair-vega-{prefix}-{suffix:016x}"));
        fs::create_dir_all(&root)
            .with_context(|| format!("create disposable runtime root {}", root.display()))?;

        Ok(Self { root, cleanup })
    }
}

impl Drop for DisposableRuntime {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

pub fn keep_runtime_requested() -> bool {
    std::env::var_os(KEEP_RUNTIME_ENV)
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
}

pub fn runtime_root_from_env() -> Option<PathBuf> {
    std::env::var_os(RUNTIME_ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn resolve_runtime_state_dir(cli_value: Option<PathBuf>, default_name: &str) -> PathBuf {
    resolve_runtime_state_dir_with_root(cli_value, runtime_root_from_env().as_deref(), default_name)
}

pub fn preferred_runtime_parent() -> PathBuf {
    preferred_runtime_parent_from(
        std::env::var_os("XDG_RUNTIME_DIR")
            .as_deref()
            .map(Path::new),
        Path::new("/dev/shm"),
        std::env::temp_dir(),
    )
}

fn resolve_runtime_state_dir_with_root(
    cli_value: Option<PathBuf>,
    runtime_root: Option<&Path>,
    default_name: &str,
) -> PathBuf {
    if let Some(path) = cli_value {
        return path;
    }

    if let Some(root) = runtime_root {
        return root.join(default_name);
    }

    PathBuf::from(default_name)
}

fn preferred_runtime_parent_from(
    xdg_runtime_dir: Option<&Path>,
    dev_shm: &Path,
    fallback_temp_dir: PathBuf,
) -> PathBuf {
    if let Some(path) = xdg_runtime_dir.filter(|path| path.is_dir()) {
        return path.to_path_buf();
    }

    if dev_shm.is_dir() {
        return dev_shm.to_path_buf();
    }

    fallback_temp_dir
}

#[cfg(test)]
mod tests {
    use super::{
        DisposableRuntime, preferred_runtime_parent_from, resolve_runtime_state_dir_with_root,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
    };
    use tempfile::TempDir;

    #[test]
    fn resolve_runtime_state_dir_prefers_explicit_cli_value() {
        let explicit = PathBuf::from("/tmp/altair-vega-explicit");
        let resolved = resolve_runtime_state_dir_with_root(
            Some(explicit.clone()),
            Some(Path::new("/runtime-root")),
            ".altair-sync-docs",
        );
        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_runtime_state_dir_uses_runtime_root_when_present() {
        let resolved = resolve_runtime_state_dir_with_root(
            None,
            Some(Path::new("/runtime-root")),
            ".altair-sync-docs",
        );
        assert_eq!(
            resolved,
            Path::new("/runtime-root").join(".altair-sync-docs")
        );
    }

    #[test]
    fn resolve_runtime_state_dir_falls_back_to_local_default() {
        let resolved = resolve_runtime_state_dir_with_root(None, None, ".altair-sync-docs");
        assert_eq!(resolved, PathBuf::from(".altair-sync-docs"));
    }

    #[test]
    fn preferred_runtime_parent_prefers_xdg_runtime_dir() {
        let temp = TempDir::new().unwrap();
        let xdg_runtime_dir = temp.path().join("run-user-1000");
        fs::create_dir(&xdg_runtime_dir).unwrap();
        let fallback = temp.path().join("fallback-root");
        let resolved = preferred_runtime_parent_from(
            Some(&xdg_runtime_dir),
            &temp.path().join("dev-shm"),
            fallback,
        );
        assert_eq!(resolved, xdg_runtime_dir);
    }

    #[test]
    fn disposable_runtime_cleans_up_on_drop() {
        let root_parent = TempDir::new().unwrap();
        let runtime_path = {
            let runtime =
                DisposableRuntime::create_in_with_cleanup(root_parent.path(), "test", true)
                    .unwrap();
            let marker = runtime.path().join("marker.txt");
            fs::write(&marker, b"marker").unwrap();
            assert!(runtime.path().exists());
            runtime.path().to_path_buf()
        };

        assert!(!runtime_path.exists());
    }

    #[test]
    fn disposable_runtime_can_be_kept_for_debugging() {
        let root_parent = TempDir::new().unwrap();
        let runtime_path = {
            let runtime =
                DisposableRuntime::create_in_with_cleanup(root_parent.path(), "test", false)
                    .unwrap();
            runtime.path().to_path_buf()
        };

        assert!(runtime_path.exists());
        fs::remove_dir_all(runtime_path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn launcher_script_has_valid_posix_syntax() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/startup.sh");
        let status = Command::new("sh").arg("-n").arg(&script).status().unwrap();
        assert!(
            status.success(),
            "shell syntax check failed for {}",
            script.display()
        );
    }

    #[cfg(unix)]
    #[test]
    fn launcher_script_runs_downloaded_binary_and_cleans_workspace() {
        if !command_exists("curl") && !command_exists("wget") {
            return;
        }

        let temp = TempDir::new().unwrap();
        let probe_dir = temp.path().join("probe");
        fs::create_dir_all(&probe_dir).unwrap();

        let stub = temp.path().join("stub.sh");
        fs::write(
            &stub,
            format!(
                "#!/bin/sh\nset -eu\nout_dir=\"$1\"\nprintf '%s\\n' \"$0\" > \"$out_dir/exe-path.txt\"\nprintf '%s\\n' \"${{{}:-}}\" > \"$out_dir/runtime-root.txt\"\ntouch \"${{{}}}/child-marker\"\nprintf '%s\\n' \"$2\" > \"$out_dir/payload.txt\"\n",
                super::RUNTIME_ROOT_ENV,
                super::RUNTIME_ROOT_ENV,
            ),
        )
        .unwrap();

        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&stub).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&stub, permissions).unwrap();

        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/startup.sh");
        let file_url = format!("file://{}", stub.display());
        let status = Command::new("sh")
            .arg(&script)
            .arg("--url")
            .arg(&file_url)
            .arg("--")
            .arg(&probe_dir)
            .arg("payload-ok")
            .status()
            .unwrap();

        assert!(status.success(), "launcher exited unsuccessfully");

        let exe_path = fs::read_to_string(probe_dir.join("exe-path.txt")).unwrap();
        let exe_path = PathBuf::from(exe_path.trim());
        let runtime_root = fs::read_to_string(probe_dir.join("runtime-root.txt")).unwrap();
        let runtime_root = PathBuf::from(runtime_root.trim());
        let payload = fs::read_to_string(probe_dir.join("payload.txt")).unwrap();

        assert_eq!(payload.trim(), "payload-ok");
        assert!(!exe_path.exists(), "downloaded executable was not removed");
        assert!(!runtime_root.exists(), "runtime workspace was not removed");
    }

    #[test]
    fn powershell_launcher_declares_runtime_contract() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/startup.ps1");
        let contents = fs::read_to_string(&script).unwrap();
        assert!(contents.contains(super::RUNTIME_ROOT_ENV));
        assert!(contents.contains(super::KEEP_RUNTIME_ENV));
        assert!(contents.contains("Remove-Item"));
        assert!(contents.contains("Invoke-WebRequest"));
    }

    #[test]
    fn powershell_launcher_has_valid_syntax_when_pwsh_is_available() {
        if !pwsh_available() {
            return;
        }

        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/startup.ps1");
        let status = Command::new("pwsh")
            .arg("-NoProfile")
            .arg("-File")
            .arg(&script)
            .arg("-Help")
            .status()
            .unwrap();
        assert!(
            status.success(),
            "powershell syntax/help check failed for {}",
            script.display()
        );
    }

    #[cfg(unix)]
    fn command_exists(name: &str) -> bool {
        Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {name} >/dev/null 2>&1"))
            .status()
            .is_ok_and(|status| status.success())
    }

    fn pwsh_available() -> bool {
        Command::new("pwsh")
            .arg("-NoProfile")
            .arg("-Command")
            .arg("$PSVersionTable.PSVersion.ToString()")
            .status()
            .is_ok_and(|status| status.success())
    }
}
