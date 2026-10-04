use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::domain::distribution::{self, DistributionError, GitHubRelease, ReleaseManifest};
use serde::de::DeserializeOwned;

use crate::control_plane::update::{UpdateStartError, UpdateStateOwner};
use crate::domain::{
    ErrorCategory, GitHubRepository, OperationError, ProductVersion, ReleaseDiscovery,
    UpdateCheckTrigger,
};

const UPDATE_CHECK_GLOBAL_TIMEOUT: Duration = Duration::from_secs(5);
const UPDATE_CHECK_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const UPDATE_CHECK_RETRY_DELAY: Duration = Duration::from_millis(250);
const UPDATE_CHECK_MAX_ATTEMPTS: u8 = 2;
const UPDATE_RESPONSE_MAX_BYTES: u64 = 64 * 1024;

pub trait ReleaseSource: Send + Sync {
    fn latest(&self, repository: &GitHubRepository) -> Result<ReleaseDiscovery, UpdateFetchError>;
}

#[derive(Clone)]
pub struct GitHubReleaseSource {
    agent: ureq::Agent,
}

impl Default for GitHubReleaseSource {
    fn default() -> Self {
        use ureq::tls::{RootCerts, TlsConfig};

        let config = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_connect(Some(UPDATE_CHECK_CONNECT_TIMEOUT))
            .timeout_global(Some(UPDATE_CHECK_GLOBAL_TIMEOUT))
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .build();
        Self {
            agent: config.new_agent(),
        }
    }
}

impl GitHubReleaseSource {
    fn json<T: DeserializeOwned>(&self, url: &str) -> Result<T, UpdateFetchError> {
        let mut response = self
            .agent
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header(
                "User-Agent",
                concat!("LocalBridge/", env!("CARGO_PKG_VERSION")),
            )
            .call()
            .map_err(UpdateFetchError::from_transport)?;
        let body = response
            .body_mut()
            .with_config()
            .limit(UPDATE_RESPONSE_MAX_BYTES)
            .read_to_string()
            .map_err(UpdateFetchError::from_transport)?;
        serde_json::from_str(&body).map_err(|_| UpdateFetchError::InvalidResponse)
    }

    fn published(
        &self,
        repository: &GitHubRepository,
        api_url: &str,
    ) -> Result<(GitHubRelease, ReleaseManifest), UpdateFetchError> {
        let release: GitHubRelease = self.json(api_url)?;
        let url = release
            .manifest_url(repository)
            .map_err(UpdateFetchError::from)?;
        let manifest: ReleaseManifest = self.json(&url).map_err(|error| {
            if error == UpdateFetchError::NoPublishedRelease {
                UpdateFetchError::MissingAsset
            } else {
                error
            }
        })?;
        manifest
            .validate(repository, &release)
            .map_err(UpdateFetchError::from)?;
        Ok((release, manifest))
    }

    pub fn current_extension(&self) -> Result<ReleaseManifest, UpdateFetchError> {
        let repository = GitHubRepository::official();
        let version = ProductVersion::current().to_string();
        let tag = distribution::release_tag(&version);
        let (_, manifest) = self.published(
            &repository,
            &format!(
                "https://api.github.com/repos/{}/releases/tags/{tag}",
                repository.as_str()
            ),
        )?;
        manifest
            .require_current_extension(
                &version,
                env!("LOCALBRIDGE_SOURCE_COMMIT"),
                crate::browser_connection::EXTENSION_ID.trim(),
            )
            .map_err(UpdateFetchError::from)?;
        Ok(manifest)
    }
}

impl ReleaseSource for GitHubReleaseSource {
    fn latest(&self, repository: &GitHubRepository) -> Result<ReleaseDiscovery, UpdateFetchError> {
        let (release, manifest) = self.published(repository, &repository.latest_api_url())?;
        ReleaseDiscovery::new(
            repository,
            manifest.product_version().map_err(UpdateFetchError::from)?,
            release.html_url,
        )
        .map_err(|_| UpdateFetchError::ForeignReleaseUrl)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateFetchError {
    Timeout,
    Transport,
    NoPublishedRelease,
    RateLimited,
    InvalidResponse,
    InvalidVersion,
    ForeignReleaseUrl,
    MissingAsset,
    Incompatible,
}

impl From<DistributionError> for UpdateFetchError {
    fn from(error: DistributionError) -> Self {
        match error {
            DistributionError::Unpublished => Self::NoPublishedRelease,
            DistributionError::MissingAsset => Self::MissingAsset,
            DistributionError::Invalid => Self::InvalidResponse,
            DistributionError::ForeignSource => Self::ForeignReleaseUrl,
            DistributionError::Incompatible => Self::Incompatible,
        }
    }
}

impl UpdateFetchError {
    pub fn user_message(self) -> &'static str {
        match self {
            Self::NoPublishedRelease => {
                "本项目尚未发布此版本的配套下载。请使用安装包内置扩展，或手动导入同次构建的发行 ZIP。"
            }
            Self::MissingAsset => {
                "此发行缺少配套清单或附件，请使用内置扩展，或联系项目维护者补齐发行文件。"
            }
            Self::Incompatible => {
                "发行扩展与当前应用的版本、来源提交或协议不匹配。请使用内置扩展，或下载同次发行的新版安装包。"
            }
            Self::ForeignReleaseUrl => "发行文件不属于本项目，已拒绝打开。请使用本项目的配套下载。",
            Self::Timeout | Self::Transport => {
                "无法读取配套下载信息，请检查网络后重试；内置扩展仍可准备。"
            }
            Self::RateLimited => "GitHub 暂时限制查询次数，请稍后重试；内置扩展仍可准备。",
            Self::InvalidResponse | Self::InvalidVersion => {
                "发行清单损坏或格式不兼容，请使用内置扩展，或联系项目维护者。"
            }
        }
    }

    fn from_transport(error: ureq::Error) -> Self {
        match error {
            ureq::Error::Timeout(_) => Self::Timeout,
            ureq::Error::StatusCode(404) => Self::NoPublishedRelease,
            ureq::Error::StatusCode(403 | 429) => Self::RateLimited,
            _ => Self::Transport,
        }
    }

    fn retryable(self) -> bool {
        matches!(self, Self::Timeout | Self::Transport | Self::RateLimited)
    }

    fn operation_error(self) -> OperationError {
        let (code, category, message) = match self {
            Self::Timeout => (
                "Update.Timeout",
                ErrorCategory::Timeout,
                "update check timed out",
            ),
            Self::Transport => (
                "Update.TransportUnavailable",
                ErrorCategory::Unavailable,
                "update service is unavailable",
            ),
            Self::NoPublishedRelease => (
                "Update.NoPublishedRelease",
                ErrorCategory::Unavailable,
                self.user_message(),
            ),
            Self::RateLimited => (
                "Update.RateLimited",
                ErrorCategory::Unavailable,
                "update service rate limit was reached",
            ),
            Self::InvalidResponse => (
                "Update.InvalidResponse",
                ErrorCategory::Unavailable,
                "update service returned an invalid response",
            ),
            Self::InvalidVersion => (
                "Update.InvalidVersion",
                ErrorCategory::Unavailable,
                "latest release has an invalid version",
            ),
            Self::MissingAsset => (
                "Update.MissingAsset",
                ErrorCategory::Unavailable,
                self.user_message(),
            ),
            Self::Incompatible => (
                "Update.Incompatible",
                ErrorCategory::Unavailable,
                self.user_message(),
            ),
            Self::ForeignReleaseUrl => (
                "Update.ForeignReleaseUrl",
                ErrorCategory::Authorization,
                "latest release URL is outside the configured repository",
            ),
        };
        OperationError::new(code, category, message, self.retryable())
    }
}

#[derive(Clone)]
pub struct UpdateChecker {
    owner: UpdateStateOwner,
    source: Arc<dyn ReleaseSource>,
}

impl std::fmt::Debug for UpdateChecker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UpdateChecker")
            .field("state", &self.owner.snapshot())
            .finish()
    }
}

impl UpdateChecker {
    pub fn production(owner: UpdateStateOwner) -> Self {
        Self::new(owner, Arc::new(GitHubReleaseSource::default()))
    }

    pub fn new(owner: UpdateStateOwner, source: Arc<dyn ReleaseSource>) -> Self {
        Self { owner, source }
    }

    pub fn owner(&self) -> UpdateStateOwner {
        self.owner.clone()
    }

    pub fn start(&self, trigger: UpdateCheckTrigger) -> Result<(), UpdateStartError> {
        let mut lease = self.owner.begin(trigger)?;
        let repository = self
            .owner
            .repository()
            .ok_or(UpdateStartError::SourceUnavailable)?;
        let source = Arc::clone(&self.source);
        thread::Builder::new()
            .name("localbridge-update-check".into())
            .spawn(move || {
                for attempt in 1..=UPDATE_CHECK_MAX_ATTEMPTS {
                    if attempt > 1 {
                        thread::sleep(UPDATE_CHECK_RETRY_DELAY);
                        if lease.set_attempt(attempt).is_err() {
                            return;
                        }
                    }
                    match source.latest(&repository) {
                        Ok(release) => {
                            let _ = lease.succeed(release);
                            return;
                        }
                        Err(error) if error.retryable() && attempt < UPDATE_CHECK_MAX_ATTEMPTS => {}
                        Err(error) => {
                            let _ = lease.fail(error.operation_error());
                            return;
                        }
                    }
                }
            })
            .map(|_| ())
            .map_err(|_| UpdateStartError::ThreadSpawnFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::UpdateLifecycle;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::Instant;

    struct BlockingSource {
        entered: Mutex<mpsc::Sender<()>>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl ReleaseSource for BlockingSource {
        fn latest(
            &self,
            repository: &GitHubRepository,
        ) -> Result<ReleaseDiscovery, UpdateFetchError> {
            let _ = self.entered.lock().unwrap().send(());
            let _ = self.release.lock().unwrap().recv();
            Ok(ReleaseDiscovery::new(
                repository,
                ProductVersion::parse("1.0.0").unwrap(),
                repository.releases_url(),
            )
            .unwrap())
        }
    }

    struct RetrySource(Mutex<u8>);

    impl ReleaseSource for RetrySource {
        fn latest(
            &self,
            repository: &GitHubRepository,
        ) -> Result<ReleaseDiscovery, UpdateFetchError> {
            let mut calls = self.0.lock().unwrap();
            *calls += 1;
            if *calls == 1 {
                return Err(UpdateFetchError::Transport);
            }
            ReleaseDiscovery::new(
                repository,
                ProductVersion::parse("1.1.0").unwrap(),
                format!("{}/tag/v1.1.0", repository.releases_url()),
            )
            .map_err(|_| UpdateFetchError::ForeignReleaseUrl)
        }
    }

    fn owner() -> UpdateStateOwner {
        UpdateStateOwner::new(
            ProductVersion::parse("1.0.0").unwrap(),
            Some(GitHubRepository::new("owner/repo").unwrap()),
        )
    }

    #[test]
    fn startup_check_returns_before_network_and_completes_terminally() {
        let owner = owner();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let checker = UpdateChecker::new(
            owner.clone(),
            Arc::new(BlockingSource {
                entered: Mutex::new(entered_tx),
                release: Mutex::new(release_rx),
            }),
        );
        let started = Instant::now();
        checker.start(UpdateCheckTrigger::Startup).unwrap();
        assert!(started.elapsed() < Duration::from_millis(100));
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(owner.snapshot(), UpdateLifecycle::Checking { .. }));
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while matches!(owner.snapshot(), UpdateLifecycle::Checking { .. }) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert!(matches!(owner.snapshot(), UpdateLifecycle::Current { .. }));
    }

    #[test]
    fn retryable_failure_is_bounded_and_second_attempt_can_succeed() {
        let owner = owner();
        let checker = UpdateChecker::new(owner.clone(), Arc::new(RetrySource(Mutex::new(0))));
        checker.start(UpdateCheckTrigger::Manual).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while matches!(owner.snapshot(), UpdateLifecycle::Checking { .. }) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert!(matches!(
            owner.snapshot(),
            UpdateLifecycle::Available { latest_version, .. }
                if latest_version == ProductVersion::parse("1.1.0").unwrap()
        ));
    }
}
