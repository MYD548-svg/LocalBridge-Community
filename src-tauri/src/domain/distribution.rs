//! Shared product distribution contract. A GitHub tag is not an application version.
use super::{GitHubRepository, ProductVersion};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseConfiguration {
    pub schema_version: u32,
    pub repository: String,
    pub channel: String,
    pub tag_suffix: String,
    pub manifest_asset: String,
    pub application_compatibility: String,
}

pub fn configuration() -> &'static ReleaseConfiguration {
    static CONFIGURATION: OnceLock<ReleaseConfiguration> = OnceLock::new();
    CONFIGURATION.get_or_init(|| {
        let config: ReleaseConfiguration =
            serde_json::from_str(include_str!("../../../product-release.json"))
                .expect("product release configuration must be valid");
        assert_eq!(config.schema_version, 1);
        assert_eq!(config.channel, "community");
        assert!(GitHubRepository::new(&config.repository).is_ok());
        assert!(safe_asset_name(&config.manifest_asset));
        assert_eq!(config.tag_suffix, "-community.1");
        assert!(semver::VersionReq::parse(&config.application_compatibility).is_ok());
        config
    })
}

pub fn release_tag(version: &str) -> String {
    format!("v{version}{}", configuration().tag_suffix)
}

pub fn safe_asset_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 180
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

pub fn numeric_version(value: &str) -> bool {
    semver::Version::parse(value).is_ok_and(|version| {
        version.pre.is_empty()
            && version.build.is_empty()
            && [version.major, version.minor, version.patch]
                .into_iter()
                .all(|part| part <= 65535)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseAsset {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

impl ReleaseAsset {
    fn valid(&self) -> bool {
        safe_asset_name(&self.name)
            && self.sha256.len() == 64
            && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            && self.size > 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseExtension {
    pub version: String,
    pub protocol: u32,
    pub extension_id: String,
    pub application: String,
    pub asset: ReleaseAsset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub repository: String,
    pub channel: String,
    pub tag: String,
    pub source_commit: String,
    pub application_version: String,
    pub installer: ReleaseAsset,
    pub extension: ReleaseExtension,
}

#[derive(Debug, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub html_url: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionError {
    Unpublished,
    MissingAsset,
    Invalid,
    ForeignSource,
    Incompatible,
}

impl GitHubRelease {
    pub fn manifest_url(&self, repository: &GitHubRepository) -> Result<String, DistributionError> {
        if self.draft || self.prerelease {
            return Err(DistributionError::Unpublished);
        }
        if !repository.owns_release_url(&self.html_url)
            || self.html_url != format!("{}/tag/{}", repository.releases_url(), self.tag_name)
            || !safe_asset_name(&self.tag_name)
        {
            return Err(DistributionError::ForeignSource);
        }
        self.asset_url(repository, &configuration().manifest_asset, None)
    }

    fn asset_url(
        &self,
        repository: &GitHubRepository,
        name: &str,
        size: Option<u64>,
    ) -> Result<String, DistributionError> {
        let mut assets = self.assets.iter().filter(|asset| asset.name == name);
        let asset = assets.next().ok_or(DistributionError::MissingAsset)?;
        if assets.next().is_some() || size.is_some_and(|size| size != asset.size) {
            return Err(DistributionError::Invalid);
        }
        let expected = format!(
            "{}/download/{}/{name}",
            repository.releases_url(),
            self.tag_name
        );
        if asset.browser_download_url != expected {
            return Err(DistributionError::ForeignSource);
        }
        Ok(expected)
    }
}

impl ReleaseManifest {
    pub fn validate(
        &self,
        repository: &GitHubRepository,
        release: &GitHubRelease,
    ) -> Result<(), DistributionError> {
        release.manifest_url(repository)?;
        if self.repository != repository.as_str() || self.repository != configuration().repository {
            return Err(DistributionError::ForeignSource);
        }
        if self.schema_version != 1
            || self.channel != configuration().channel
            || !numeric_version(&self.application_version)
            || self.tag != release_tag(&self.application_version)
            || self.tag != release.tag_name
            || self.source_commit.len() != 40
            || !self
                .source_commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || !self.installer.valid()
            || self.installer.name
                != format!("LocalBridge_{}_x64-setup.exe", self.application_version)
            || !self.extension.asset.valid()
            || self.extension.asset.size > 32 * 1024 * 1024
            || !numeric_version(&self.extension.version)
            || self.extension.asset.name
                != format!("LocalBridge-ChatGPT-Web-v{}.zip", self.extension.version)
        {
            return Err(DistributionError::Invalid);
        }
        if self.extension.protocol == 0
            || self.extension.extension_id.len() != 32
            || !self
                .extension
                .extension_id
                .bytes()
                .all(|byte| (b'a'..=b'p').contains(&byte))
            || !semver::VersionReq::parse(&self.extension.application)
                .map_err(|_| DistributionError::Invalid)?
                .matches(
                    &semver::Version::parse(&self.application_version)
                        .map_err(|_| DistributionError::Invalid)?,
                )
        {
            return Err(DistributionError::Incompatible);
        }
        release.asset_url(repository, &self.installer.name, Some(self.installer.size))?;
        release.asset_url(
            repository,
            &self.extension.asset.name,
            Some(self.extension.asset.size),
        )?;
        Ok(())
    }

    pub fn require_current_extension(
        &self,
        version: &str,
        source_commit: &str,
        extension_id: &str,
    ) -> Result<(), DistributionError> {
        if self.application_version != version
            || self.source_commit != source_commit
            || self.extension.protocol != 1
            || self.extension.extension_id != extension_id
            || self.extension.application != configuration().application_compatibility
        {
            return Err(DistributionError::Incompatible);
        }
        Ok(())
    }

    pub fn product_version(&self) -> Result<ProductVersion, DistributionError> {
        ProductVersion::parse(&self.application_version).map_err(|_| DistributionError::Invalid)
    }

    pub fn extension_url(&self, repository: &GitHubRepository) -> String {
        format!(
            "{}/download/{}/{}",
            repository.releases_url(),
            self.tag,
            self.extension.asset.name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (GitHubRepository, GitHubRelease, ReleaseManifest) {
        let repository = GitHubRepository::official();
        let tag = release_tag("0.1.6");
        let asset = |name: &str, size| GitHubAsset {
            name: name.into(),
            browser_download_url: format!("{}/download/{tag}/{name}", repository.releases_url()),
            size,
        };
        let release = GitHubRelease {
            tag_name: tag.clone(),
            html_url: format!("{}/tag/{tag}", repository.releases_url()),
            draft: false,
            prerelease: false,
            assets: vec![
                asset(&configuration().manifest_asset, 500),
                asset("LocalBridge_0.1.6_x64-setup.exe", 900),
                asset("LocalBridge-ChatGPT-Web-v0.1.6.zip", 600),
            ],
        };
        let manifest = ReleaseManifest {
            schema_version: 1,
            repository: repository.as_str().into(),
            channel: "community".into(),
            tag,
            source_commit: "a".repeat(40),
            application_version: "0.1.6".into(),
            installer: ReleaseAsset {
                name: "LocalBridge_0.1.6_x64-setup.exe".into(),
                sha256: "b".repeat(64),
                size: 900,
            },
            extension: ReleaseExtension {
                version: "0.1.6".into(),
                protocol: 1,
                extension_id: "a".repeat(32),
                application: configuration().application_compatibility.clone(),
                asset: ReleaseAsset {
                    name: "LocalBridge-ChatGPT-Web-v0.1.6.zip".into(),
                    sha256: "c".repeat(64),
                    size: 600,
                },
            },
        };
        (repository, release, manifest)
    }

    #[test]
    fn product_version_comes_from_manifest_and_downloads_are_exactly_bound() {
        let (repository, release, manifest) = fixture();
        manifest.validate(&repository, &release).unwrap();
        assert_eq!(
            manifest.product_version().unwrap(),
            ProductVersion::parse("0.1.6").unwrap()
        );
        assert!(manifest.product_version().unwrap() > ProductVersion::parse("0.1.5").unwrap());
        assert!(!numeric_version("0.1.6-community.1"));
        assert!(!numeric_version("0.1.65536"));
    }

    #[test]
    fn foreign_missing_prerelease_and_mixed_packages_cannot_be_offered() {
        let (repository, mut release, mut manifest) = fixture();
        let id = "a".repeat(32);
        release.assets[2].browser_download_url =
            "https://github.com/zephyr7030/LocalBridge/releases/download/v0.1.6/extension.zip"
                .into();
        assert_eq!(
            manifest.validate(&repository, &release),
            Err(DistributionError::ForeignSource)
        );
        release.assets.pop();
        assert_eq!(
            manifest.validate(&repository, &release),
            Err(DistributionError::MissingAsset)
        );
        release.prerelease = true;
        assert_eq!(
            release.manifest_url(&repository),
            Err(DistributionError::Unpublished)
        );
        release.prerelease = false;
        manifest.extension.protocol = 2;
        assert_eq!(
            manifest.require_current_extension("0.1.6", &"a".repeat(40), &id),
            Err(DistributionError::Incompatible)
        );
    }

    #[test]
    fn main_application_updates_can_change_extension_protocol_but_current_downloads_cannot() {
        let (repository, release, mut manifest) = fixture();
        let id = "a".repeat(32);
        manifest
            .require_current_extension("0.1.6", &"a".repeat(40), &id)
            .unwrap();
        assert_eq!(
            manifest.require_current_extension("0.1.5", &"a".repeat(40), &id),
            Err(DistributionError::Incompatible)
        );
        assert_eq!(
            manifest.require_current_extension("0.1.6", &"b".repeat(40), &id),
            Err(DistributionError::Incompatible)
        );
        manifest.extension.protocol = 2;
        manifest.validate(&repository, &release).unwrap();
        assert_eq!(
            manifest.require_current_extension("0.1.6", &"a".repeat(40), &id),
            Err(DistributionError::Incompatible)
        );
    }
}
