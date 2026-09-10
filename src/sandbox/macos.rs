use std::ffi::CString;
use std::path::Path;

use super::SandboxInfo;

const SYSTEM_READ_PATHS: &[&str] = &[
    "/usr", "/bin", "/sbin", "/etc", "/opt", "/System", "/Library", "/nix", "/private", "/var",
];

const TEMP_WRITE_PATHS: &[&str] = &["/tmp", "/var/tmp", "/private/tmp", "/private/var/tmp"];

const DENIED_PATHS: &[&str] = &[
    ".ssh",
    ".aws",
    ".azure",
    ".gcloud",
    ".config/gcloud",
    ".docker/config.json",
    ".git-credentials",
    ".gitconfig",
    ".gnupg",
    ".bash_history",
    ".zsh_history",
    ".zshrc",
    ".bashrc",
    ".bash_profile",
    ".config",
    "Library/Application Support/Google",
    "Library/Application Support/Firefox",
    "Library/Application Support/BraveSoftware",
    "Library/Application Support/Chromium",
    "Library/Keychains",
    ".npmrc",
    ".yarnrc",
    ".yarnrc.yml",
    ".pypirc",
    ".kube",
    ".netrc",
];

fn push_file_read(sb: &mut String, path: &str) {
    sb.push_str(&format!(
        "(allow file-read* (subpath \"{}\"))\n",
        escape_path(path)
    ));
}

fn push_file_write(sb: &mut String, path: &str) {
    sb.push_str(&format!(
        "(allow file-write* (subpath \"{}\"))\n",
        escape_path(path)
    ));
}

fn push_file_read_write(sb: &mut String, path: &str) {
    push_file_read(sb, path);
    push_file_write(sb, path);
}

fn generate_profile(worktree_path: &str, block_network: bool) -> String {
    let mut sb = String::with_capacity(4096);

    sb.push_str("(version 1)\n");

    sb.push_str("(deny default)\n");

    sb.push_str("(allow process-exec*)\n");
    sb.push_str("(allow process-fork)\n");

    sb.push_str("(allow process-info*)\n");

    sb.push_str("(allow sysctl-read)\n");

    sb.push_str("(allow ipc-posix-shm*)\n");

    sb.push_str("(allow mach-lookup)\n");
    sb.push_str("(allow mach-per-user-lookup)\n");
    sb.push_str("(allow mach-task-name)\n");

    sb.push_str("(allow file-read* (literal \"/\"))\n");

    push_file_read_write(&mut sb, worktree_path);

    if let Ok(resolved) = Path::new(worktree_path).canonicalize() {
        let resolved_str = resolved.to_string_lossy();
        if resolved_str != worktree_path {
            push_file_read_write(&mut sb, &resolved_str);
        }
    }

    for path in SYSTEM_READ_PATHS {
        if Path::new(path).exists() {
            push_file_read(&mut sb, path);
        }
    }

    for path in TEMP_WRITE_PATHS {
        if Path::new(path).exists() {
            push_file_read_write(&mut sb, path);
        }
    }

    if Path::new("/dev").exists() {
        push_file_read_write(&mut sb, "/dev");
    }

    if let Ok(home) = std::env::var("HOME") {
        for denied in DENIED_PATHS {
            let denied_path = format!("{}/{}", home, denied);
            let p = Path::new(&denied_path);
            if p.exists() {
                sb.push_str(&format!(
                    "(deny file-read* (subpath \"{}\"))\n",
                    escape_path(&denied_path)
                ));
                sb.push_str(&format!(
                    "(deny file-write* (subpath \"{}\"))\n",
                    escape_path(&denied_path)
                ));
            }
        }
    }

    if block_network {
        sb.push_str("(deny network*)\n");

        sb.push_str("(allow network-outbound (remote tcp \"localhost:*\"))\n");
        sb.push_str("(allow network-inbound (local tcp \"localhost:*\"))\n");
        sb.push_str("(allow network-bind (local tcp \"localhost:*\"))\n");

        sb.push_str("(allow network-outbound (path \"/private/var/run/mDNSResponder\"))\n");
        sb.push_str("(allow network-outbound (path \"/var/run/mDNSResponder\"))\n");
    } else {
        sb.push_str("(allow network*)\n");
        sb.push_str("(allow system-socket)\n");
    }

    sb
}

fn escape_path(path: &str) -> String {
    path.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(super) fn apply(worktree_path: &str, block_network: bool) -> Result<(), String> {
    let profile = generate_profile(worktree_path, block_network);

    let profile_cstr =
        CString::new(profile.as_str()).map_err(|_| "Profile contains null byte".to_string())?;

    let mut error_ptr: *mut std::ffi::c_char = std::ptr::null_mut();
    // SAFETY: FFI call to macOS seatbelt API. The profile string is a valid CString and error_ptr is a valid pointer.
    let result = unsafe { sandbox_init(profile_cstr.as_ptr(), 0, &mut error_ptr) };

    if result != 0 {
        let err_msg = if !error_ptr.is_null() {
            // SAFETY: sandbox_init populates error_ptr with a valid null-terminated C string on error.
            let msg = unsafe { std::ffi::CStr::from_ptr(error_ptr) }
                .to_string_lossy()
                .into_owned();
            // SAFETY: error_ptr was allocated by sandbox_init and must be freed by sandbox_free_error.
            unsafe { sandbox_free_error(error_ptr) };
            msg
        } else {
            "Unknown sandbox_init error".to_string()
        };
        return Err(format!("macOS Seatbelt sandbox failed: {}", err_msg));
    }

    Ok(())
}

extern "C" {
    fn sandbox_init(
        profile: *const std::ffi::c_char,
        flags: u64,
        errorbuf: *mut *mut std::ffi::c_char,
    ) -> i32;
    fn sandbox_free_error(errorbuf: *mut std::ffi::c_char);
}

pub(super) fn check_supported() -> bool {
    true
}

pub(super) fn get_info() -> SandboxInfo {
    SandboxInfo {
        supported: true,
        platform: "macos".to_string(),
        details: "macOS Seatbelt sandbox available (sandbox_init API)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_profile_contains_worktree() {
        let profile = generate_profile("/tmp/test-worktree", false);
        assert!(profile.contains("(allow file-read* (subpath \"/tmp/test-worktree\"))"));
        assert!(profile.contains("(allow file-write* (subpath \"/tmp/test-worktree\"))"));
    }

    #[test]
    fn test_generate_profile_allows_root_read_for_dyld_cachefinder() {
        let profile = generate_profile("/tmp/test-worktree", true);
        assert!(profile.contains("(allow file-read* (literal \"/\"))"));
    }

    #[test]
    fn test_generate_profile_contains_deny_default() {
        let profile = generate_profile("/tmp/test", false);
        assert!(profile.contains("(deny default)"));
    }

    #[test]
    fn test_generate_profile_network_blocked() {
        let profile = generate_profile("/tmp/test", true);
        assert!(profile.contains("(deny network*)"));
        assert!(profile.contains("(allow network-outbound (remote tcp \"localhost:*\"))"));
    }

    #[test]
    fn test_generate_profile_network_allowed() {
        let profile = generate_profile("/tmp/test", false);
        assert!(profile.contains("(allow network*)"));
        assert!(!profile.contains("(deny network*)"));
    }

    #[test]
    fn test_check_supported() {
        assert!(check_supported());
    }

    #[test]
    fn test_escape_path_handles_special_chars() {
        assert_eq!(escape_path("/tmp/test"), "/tmp/test");
        assert!(escape_path("/tmp/\"test\"").contains('"'));
        assert!(escape_path("/tmp/\\test").contains('\\'));
    }
}
