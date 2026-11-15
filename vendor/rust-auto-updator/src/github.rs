use crate::Config;
use semver::Version;
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Release {
    pub(crate) source: String,
    pub version: Version,
    pub notes: String,
    pub page: String,
    pub(crate) executable: Asset,
    pub(crate) checksum: Option<Asset>,
}

impl Release {
    pub fn executable_name(&self) -> &str {
        &self.executable.name
    }
    pub fn executable_url(&self) -> &str {
        &self.executable.browser_download_url
    }
    pub fn executable_size(&self) -> u64 {
        self.executable.size
    }
    pub fn size(&self) -> u64 {
        self.executable_size()
    }
    pub fn checksum_url(&self) -> Option<&str> {
        self.checksum
            .as_ref()
            .map(|asset| asset.browser_download_url.as_str())
    }
    pub fn sha256_digest(&self) -> Option<&str> {
        self.executable
            .digest
            .as_deref()
            .and_then(|value| value.strip_prefix("sha256:"))
    }
}

pub(crate) fn asset_digest(asset: &Asset) -> Result<Option<String>, String> {
    let Some(digest) = &asset.digest else {
        return Ok(None);
    };
    let digest = digest
        .strip_prefix("sha256:")
        .ok_or("The release asset has an invalid SHA-256 digest.")?;
    if digest.len() != 64 || !digest.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err("The release asset has an invalid SHA-256 digest.".into());
    }
    Ok(Some(digest.to_ascii_lowercase()))
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Asset {
    pub(crate) name: String,
    pub(crate) browser_download_url: String,
    pub(crate) size: u64,
    pub(crate) digest: Option<String>,
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

pub(crate) fn agent(config: &Config) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(5)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(120)))
        .timeout_per_read(Some(Duration::from_secs(20)))
        .user_agent(format!(
            "{}/{}",
            config.product_name, config.current_version
        ))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent()
}

pub(crate) fn read_text(agent: &ureq::Agent, url: &str, limit: u64) -> Result<String, String> {
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

pub(crate) fn network_error(e: ureq::Error) -> String {
    if matches!(e, ureq::Error::StatusCode(403 | 429)) {
        "GitHub's request limit was reached. Try again later.".into()
    } else {
        format!("Could not contact GitHub: {e}")
    }
}

pub fn check_latest(config: &Config) -> Result<Option<Release>, String> {
    config.validate()?;
    parse_release(
        &read_text(&agent(config), &config.api_url(), 1024 * 1024)?,
        config,
    )
}

pub fn parse_release(json: &str, config: &Config) -> Result<Option<Release>, String> {
    config.validate()?;
    let repo = config.repository_url();
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
    if !version.pre.is_empty() || version.cmp_precedence(&config.current_version).is_le() {
        return Ok(None);
    }
    let tag = format!("v{version}");
    if release.tag_name != tag || release.html_url != format!("{repo}/releases/tag/{tag}") {
        return Err("The release does not belong to the expected GitHub repository.".into());
    }
    let asset = |name: &str| -> Result<Asset, String> {
        let mut matches = release.assets.iter().filter(|a| a.name == name);
        let asset = matches
            .next()
            .ok_or_else(|| format!("The release is missing {name}."))?;
        if matches.next().is_some()
            || asset.browser_download_url != format!("{repo}/releases/download/{tag}/{name}")
        {
            return Err("The release contains an unexpected download address.".into());
        }
        Ok(asset.clone())
    };
    let executable_name = config
        .executable_names(&version)
        .into_iter()
        .find(|name| {
            release
                .assets
                .iter()
                .any(|candidate| candidate.name == *name)
        })
        .ok_or_else(|| {
            format!(
                "The release is missing {}.",
                config.executable_name(&version)
            )
        })?;
    let executable = asset(&executable_name)?;
    if executable.size < 1024 || executable.size > config.max_executable_size {
        return Err("The release executable has an unexpected size.".into());
    }
    let checksum_name = config.checksum_filename(&executable_name);
    let checksum = if release
        .assets
        .iter()
        .any(|candidate| candidate.name == checksum_name)
    {
        Some(asset(&checksum_name)?)
    } else if config.allow_asset_digest && asset_digest(&executable)?.is_some() {
        None
    } else {
        return Err(format!("The release is missing {checksum_name}."));
    };
    // A supplied digest is always validated, even with a separate checksum file.
    asset_digest(&executable)?;
    Ok(Some(Release {
        source: repo.clone(),
        version,
        notes: release
            .body
            .unwrap_or_default()
            .chars()
            .take(12000)
            .collect(),
        page: release.html_url,
        executable,
        checksum,
    }))
}
