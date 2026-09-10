/// Kernel-enforced sandboxing for execution sessions.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
mod macos;

#[derive(Debug, Clone)]
pub struct SandboxInfo {
    pub supported: bool,
    pub platform: String,
    pub details: String,
}

/// Irreversible: once applied, the process cannot lift the sandbox.
pub fn apply(worktree_path: &str, block_network: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        linux::apply(worktree_path, block_network)
    }

    #[cfg(target_os = "macos")]
    {
        macos::apply(worktree_path, block_network)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (worktree_path, block_network);
        Err(format!(
            "Sandboxing not supported on '{}' (requires Linux with Landlock or macOS with Seatbelt)",
            std::env::consts::OS
        ))
    }
}

pub fn check_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::check_supported()
    }

    #[cfg(target_os = "macos")]
    {
        macos::check_supported()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

pub fn get_info() -> SandboxInfo {
    #[cfg(target_os = "linux")]
    {
        linux::get_info()
    }

    #[cfg(target_os = "macos")]
    {
        macos::get_info()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        SandboxInfo {
            supported: false,
            platform: std::env::consts::OS.to_string(),
            details: format!(
                "Platform '{}' is not supported. Requires Linux (5.13+) with Landlock or macOS with Seatbelt.",
                std::env::consts::OS
            ),
        }
    }
}
