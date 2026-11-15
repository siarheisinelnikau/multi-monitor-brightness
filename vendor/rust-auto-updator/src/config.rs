use semver::Version;

/// PE machine type to accept after checksum verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Architecture {
    X64,
    X86,
    Arm64,
}

impl Architecture {
    pub(crate) fn machine(self) -> u16 {
        match self {
            Self::X64 => 0x8664,
            Self::X86 => 0x014c,
            Self::Arm64 => 0xaa64,
        }
    }
}

/// Host-specific restart behavior. No application flags are assumed.
#[derive(Clone, Debug)]
pub struct InstallOptions {
    pub restart_args: Vec<String>,
    pub rollback_args: Vec<String>,
    pub stage_prefix: String,
    pub helper_name: String,
    /// Wait for first-frame acknowledgement before discarding rollback files.
    /// A child that exits before acknowledgement is rolled back. A live child
    /// that reaches the timeout is left running with its backup preserved.
    pub startup_health_timeout: Option<std::time::Duration>,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            restart_args: Vec::new(),
            rollback_args: Vec::new(),
            stage_prefix: ".rust-auto-updator-".into(),
            helper_name: "update-helper.exe".into(),
            startup_health_timeout: None,
        }
    }
}

/// Product configuration. Create it with the consuming application's version,
/// never this library's Cargo version. Stable releases must use `vSEMVER` tags.
#[derive(Clone, Debug)]
pub struct Config {
    pub owner: String,
    pub repository: String,
    pub current_version: Version,
    pub product_name: String,
    pub architecture: Architecture,
    /// Optional PE subsystem restriction: 2 for Windows GUI, 3 for console.
    pub expected_subsystem: Option<u16>,
    /// Empty for one architecture; e.g. `-x64` for multi-architecture releases.
    pub asset_suffix: String,
    /// Ordered legacy suffixes to try only when the primary filename is absent.
    pub asset_suffixes: Vec<String>,
    /// Additional historical filenames; `{version}` expands to the release version.
    /// The canonical versioned product filename is always preferred.
    pub legacy_executable_names: Vec<String>,
    /// A safe filename, optionally containing `{executable}` for per-asset sums.
    pub checksum_name: String,
    /// Accept GitHub's SHA-256 asset digest when no checksum asset is supplied.
    /// If both are supplied they must agree; a malformed checksum never falls back.
    pub allow_asset_digest: bool,
    pub max_executable_size: u64,
    pub install: InstallOptions,
}

impl Config {
    pub fn new(
        owner: &str,
        repository: &str,
        version: &str,
        product: &str,
    ) -> Result<Self, String> {
        let config = Self {
            owner: owner.into(),
            repository: repository.into(),
            current_version: Version::parse(version).map_err(|e| e.to_string())?,
            product_name: product.into(),
            architecture: if cfg!(target_arch = "aarch64") {
                Architecture::Arm64
            } else if cfg!(target_arch = "x86") {
                Architecture::X86
            } else {
                Architecture::X64
            },
            expected_subsystem: None,
            asset_suffix: String::new(),
            asset_suffixes: Vec::new(),
            legacy_executable_names: Vec::new(),
            checksum_name: "SHA256SUMS.txt".into(),
            allow_asset_digest: false,
            max_executable_size: 64 * 1024 * 1024,
            install: InstallOptions::default(),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn repository_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.repository)
    }

    pub fn api_url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/releases/latest",
            self.owner, self.repository
        )
    }

    pub fn executable_name(&self, version: &Version) -> String {
        format!("{}-v{version}{}.exe", self.product_name, self.asset_suffix)
    }

    pub fn executable_names(&self, version: &Version) -> Vec<String> {
        let mut names = vec![self.executable_name(version)];
        for suffix in &self.asset_suffixes {
            let name = format!("{}-v{version}{suffix}.exe", self.product_name);
            if !names.contains(&name) {
                names.push(name);
            }
        }
        for template in &self.legacy_executable_names {
            let name = template.replace("{version}", &version.to_string());
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    pub fn checksum_filename(&self, executable: &str) -> String {
        self.checksum_name.replace("{executable}", executable)
    }

    pub fn validate(&self) -> Result<(), String> {
        let safe = |text: &str| {
            !text.is_empty()
                && text != "."
                && text != ".."
                && text
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        };
        if !safe(&self.owner)
            || self.install.startup_health_timeout.is_some_and(|timeout| {
                timeout.is_zero() || timeout > std::time::Duration::from_secs(300)
            })
            || self
                .expected_subsystem
                .is_some_and(|value| ![2, 3].contains(&value))
            || !safe(&self.repository)
            || !safe(&self.product_name)
            || (!self.asset_suffix.is_empty() && !safe(&self.asset_suffix))
            || self
                .asset_suffixes
                .iter()
                .any(|suffix| !suffix.is_empty() && !safe(suffix))
            || self
                .legacy_executable_names
                .iter()
                .any(|name| !safe(&name.replace("{version}", "1.0.0")) || !name.ends_with(".exe"))
            || !safe(&self.checksum_filename("application.exe"))
            || self.max_executable_size < 1024
            || !safe(&self.install.stage_prefix)
            || !self.install.stage_prefix.starts_with('.')
            || !self.install.stage_prefix.ends_with('-')
            || !safe(&self.install.helper_name)
            || !self.install.helper_name.ends_with(".exe")
            || ["ready.exe", "previous.exe", "restore.exe", "failed.exe"]
                .iter()
                .any(|name| self.install.helper_name.eq_ignore_ascii_case(name))
        {
            return Err("Invalid updater configuration: repository and staging names must be plain safe filenames, and the helper must have a distinct EXE name.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_and_installer_filename_collisions() {
        assert!(Config::new("../owner", "app", "1.0.0", "App").is_err());
        let mut config = Config::new("owner", "app", "1.0.0", "App").unwrap();
        config.install.helper_name = "previous.exe".into();
        assert!(config.validate().is_err());
        config.install.helper_name = "../escape.exe".into();
        assert!(config.validate().is_err());
    }
}
