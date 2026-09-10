// Linux Landlock LSM sandbox (kernel 5.13+). Rules are simpler than a
// general LSM because the worktree path is known up front: read on system
// paths, read-write on temp dirs, optional network blocking.

use std::path::Path;
use std::sync::OnceLock;

use super::SandboxInfo;

use landlock::{
    Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd,
    RestrictSelfAttr, Ruleset, RulesetAttr, RulesetCreated, RulesetCreatedAttr, ABI,
};

// System paths: read-execute. Temp dirs: read-write. Devices: basic I/O.
const SYSTEM_READ_PATHS: &[&str] = &[
    "/usr",
    "/lib",
    "/lib64",
    "/bin",
    "/sbin",
    "/etc",
    "/opt",
    "/nix",
    "/proc",
    "/sys",
    "/usr/local",
];

const TEMP_WRITE_PATHS: &[&str] = &["/tmp", "/var/tmp", "/dev/shm"];

const DEVICE_PATHS: &[&str] = &[
    "/dev/null",
    "/dev/urandom",
    "/dev/random",
    "/dev/zero",
    "/dev/fd",
    "/dev/stdin",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/pts",
    "/dev/ptmx",
];

static CACHED_ABI: OnceLock<Result<ABI, String>> = OnceLock::new();

fn detect_abi() -> Result<ABI, String> {
    if let Some(result) = CACHED_ABI.get() {
        return result.clone();
    }

    let probe_order = [
        ABI::V8,
        ABI::V7,
        ABI::V6,
        ABI::V5,
        ABI::V4,
        ABI::V3,
        ABI::V2,
        ABI::V1,
    ];

    for &abi in &probe_order {
        let mut ruleset = Ruleset::default();
        ruleset = ruleset.set_compatibility(CompatLevel::HardRequirement);

        let handled_fs = AccessFs::from_all(abi);
        match ruleset.handle_access(handled_fs) {
            Ok(r) => ruleset = r,
            Err(_) => continue,
        }

        let handled_net = AccessNet::from_all(abi);
        if !handled_net.is_empty() {
            match ruleset.handle_access(handled_net) {
                Ok(r) => ruleset = r,
                Err(_) => continue,
            }
        }

        if ruleset.create().is_ok() {
            let _ = CACHED_ABI.set(Ok(abi));
            return Ok(abi);
        }
    }

    let msg =
        "Landlock not available. Requires Linux kernel 5.13+ with CONFIG_SECURITY_LANDLOCK=y."
            .to_string();
    let _ = CACHED_ABI.set(Err(msg.clone()));
    Err(msg)
}

fn has_basic_fs() -> bool {
    detect_abi().is_ok()
}

fn has_network() -> bool {
    detect_abi()
        .map(|abi| !AccessNet::from_all(abi).is_empty())
        .unwrap_or(false)
}

fn has_execute() -> bool {
    detect_abi()
        .map(|abi| AccessFs::from_all(abi).contains(AccessFs::Execute))
        .unwrap_or(false)
}

pub(super) fn apply(worktree_path: &str, block_network: bool) -> Result<(), String> {
    let abi = detect_abi()?;
    let worktree = Path::new(worktree_path);

    if !worktree.exists() {
        return Err(format!("Worktree path does not exist: {}", worktree_path));
    }

    let handled_fs = AccessFs::from_all(abi);
    let mut ruleset_builder = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(handled_fs)
        .map_err(|e| format!("Failed to handle filesystem access: {}", e))?;

    let needs_network_block = block_network;
    if needs_network_block && has_network() {
        let handled_net = AccessNet::from_all(abi);
        if !handled_net.is_empty() {
            ruleset_builder = ruleset_builder
                .handle_access(handled_net)
                .map_err(|e| format!("Failed to handle network access: {}", e))?;
        }
    }

    let mut ruleset = ruleset_builder
        .create()
        .map_err(|e| format!("Failed to create Landlock ruleset: {}", e))?;

    let add_path_rule = |ruleset: &mut RulesetCreated,
                         path: &Path,
                         access: BitFlags<AccessFs>|
     -> Result<(), String> {
        let path_fd =
            PathFd::new(path).map_err(|e| format!("Invalid path '{}': {}", path.display(), e))?;
        let path_beneath = PathBeneath::new(path_fd, access);
        ruleset
            .add_rule(path_beneath)
            .map_err(|e| format!("Failed to add rule for '{}': {}", path.display(), e))?;
        Ok(())
    };

    let add_resolved_path_rule = |ruleset: &mut RulesetCreated,
                                  path: &Path,
                                  access: BitFlags<AccessFs>|
     -> Result<(), String> {
        let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        add_path_rule(ruleset, &resolved, access)?;
        if resolved != path {
            add_path_rule(ruleset, path, access)?;
        }
        Ok(())
    };

    let worktree_access = (AccessFs::ReadFile
        | AccessFs::ReadDir
        | AccessFs::WriteFile
        | AccessFs::Execute
        | AccessFs::RemoveFile
        | AccessFs::RemoveDir
        | AccessFs::MakeChar
        | AccessFs::MakeDir
        | AccessFs::MakeReg
        | AccessFs::MakeSock
        | AccessFs::MakeFifo
        | AccessFs::MakeBlock
        | AccessFs::MakeSym
        | AccessFs::Refer
        | AccessFs::Truncate)
        & handled_fs;
    add_resolved_path_rule(&mut ruleset, worktree, worktree_access)?;

    let read_exec_access =
        (AccessFs::ReadFile | AccessFs::ReadDir | AccessFs::Execute) & handled_fs;
    for path_str in SYSTEM_READ_PATHS {
        let path = Path::new(path_str);
        if path.exists() {
            let _ = add_resolved_path_rule(&mut ruleset, path, read_exec_access);
        }
    }

    let read_write_dir_access = (AccessFs::ReadFile
        | AccessFs::ReadDir
        | AccessFs::WriteFile
        | AccessFs::MakeChar
        | AccessFs::MakeDir
        | AccessFs::MakeReg
        | AccessFs::RemoveFile
        | AccessFs::RemoveDir
        | AccessFs::Truncate)
        & handled_fs;
    for path_str in TEMP_WRITE_PATHS {
        let path = Path::new(path_str);
        if path.exists() {
            let _ = add_resolved_path_rule(&mut ruleset, path, read_write_dir_access);
        }
    }

    // CI runners may redirect Python's temporary files to a runner-specific
    // directory (for example RUNNER_TEMP).  pytest and subprocess capture
    // rely on being able to truncate those files after the sandbox is active.
    let runtime_temp = std::env::temp_dir();
    if runtime_temp.exists() {
        let _ = add_resolved_path_rule(&mut ruleset, &runtime_temp, read_write_dir_access);
    }
    if let Some(runner_temp) = std::env::var_os("RUNNER_TEMP") {
        let runner_temp = Path::new(&runner_temp);
        if runner_temp.exists() {
            let _ = add_resolved_path_rule(&mut ruleset, runner_temp, read_write_dir_access);
        }
    }

    let device_access = (AccessFs::ReadFile | AccessFs::WriteFile) & handled_fs;
    for path_str in DEVICE_PATHS {
        let path = Path::new(path_str);
        if path.exists() {
            let _ = add_resolved_path_rule(&mut ruleset, path, device_access);
        }
    }

    ruleset
        .all_threads(true)
        .map_err(|e| format!("Failed to configure all-thread Landlock enforcement: {}", e))?
        .restrict_self()
        .map_err(|e| format!("Failed to apply Landlock sandbox: {}", e))?;

    Ok(())
}

pub(super) fn check_supported() -> bool {
    has_basic_fs()
}

pub(super) fn get_info() -> SandboxInfo {
    match detect_abi() {
        Ok(abi) => {
            let features = describe_features(abi);
            let network = has_network();
            SandboxInfo {
                supported: true,
                platform: "linux".to_string(),
                details: format!(
                    "Landlock ABI {:?} - features: {} - network filtering: {}",
                    abi,
                    features.join(", "),
                    if network { "yes" } else { "no (requires V4+)" }
                ),
            }
        }
        Err(msg) => SandboxInfo {
            supported: false,
            platform: "linux".to_string(),
            details: msg,
        },
    }
}

fn describe_features(abi: ABI) -> Vec<String> {
    let mut features = vec!["Basic filesystem access control".to_string()];
    let available = AccessFs::from_all(abi);

    if available.contains(AccessFs::Refer) {
        features.push("rename across dirs".to_string());
    }
    if available.contains(AccessFs::Truncate) {
        features.push("truncation control".to_string());
    }
    if has_execute() {
        features.push("execute restriction".to_string());
    }
    if has_network() {
        features.push("TCP network filtering".to_string());
    }
    if !landlock::Scope::from_all(abi).is_empty() {
        features.push("signal/IPC scoping".to_string());
    }

    features
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_abi_does_not_panic() {
        let _ = detect_abi();
    }

    #[test]
    fn test_check_supported_consistent() {
        let a = check_supported();
        let b = check_supported();
        assert_eq!(a, b);
    }

    #[test]
    fn test_get_info_returns_platform() {
        let info = get_info();
        assert_eq!(info.platform, "linux");
    }

    #[test]
    fn test_landlock_abi_constants() {
        let v1_access = AccessFs::from_all(ABI::V1);
        assert!(v1_access.contains(AccessFs::ReadFile));
        assert!(v1_access.contains(AccessFs::WriteFile));
        assert!(v1_access.contains(AccessFs::Execute));
    }
}
