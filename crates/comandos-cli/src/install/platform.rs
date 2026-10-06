//! Platform classification has no installation or process side effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    LinuxNative,
    WslUbuntu,
    LinuxOther,
    Darwin,
}

impl Platform {
    pub fn detect(system: &str, kernel: &str, os_release: &str) -> Self {
        if system.trim() == "Darwin" {
            return Self::Darwin;
        }
        if system.trim() != "Linux" {
            return Self::LinuxOther;
        }
        if !kernel.to_ascii_lowercase().contains("microsoft") {
            return Self::LinuxNative;
        }
        let ubuntu = os_release
            .lines()
            .find_map(|line| line.strip_prefix("ID="))
            .is_some_and(|id| id.replace('"', "") == "ubuntu");
        if ubuntu {
            Self::WslUbuntu
        } else {
            Self::LinuxOther
        }
    }

    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            return Self::Darwin;
        }
        Self::detect(
            if cfg!(target_os = "linux") {
                "Linux"
            } else {
                "other"
            },
            &std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default(),
            &std::fs::read_to_string("/etc/os-release").unwrap_or_default(),
        )
    }
}
