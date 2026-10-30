//! The license-source adapter: the agencies' daily lists and the one live
//! lookup behind one trait (SEC-032).
//!
//! Both calls only read, so they take no idempotency key (`fetch_`, `lookup_`
//! prefixes, D-596); the throttle on live lookups is `licensing`'s. The base
//! URL is a setting: `file://` reads the fixture lists from a directory (the
//! Phase 1 stand-in), `http(s)` reads the public lists (verification only).

use std::future::Future;
use std::path::PathBuf;

use reqwest::Method;

use crate::base::{AdapterError, Auth, VendorHttp, is_safe_name};

/// Reading license records (read-only).
pub trait LicenseSource {
    /// The raw bytes of the agency daily list named `list`.
    fn fetch_daily_list(
        &self,
        list: &str,
    ) -> impl Future<Output = Result<Vec<u8>, AdapterError>> + Send;

    /// The raw answer of one live lookup of license `number`.
    fn lookup_license(
        &self,
        number: &str,
    ) -> impl Future<Output = Result<Vec<u8>, AdapterError>> + Send;
}

/// The license-source client: `file://` fixtures (`<dir>/<list>`,
/// `<dir>/lookup/<number>`) or the public lists under one base URL, same paths.
#[derive(Debug, Clone)]
pub struct LicenseLists(Lists);

#[derive(Debug, Clone)]
enum Lists {
    Fixtures(PathBuf),
    Http(VendorHttp),
}

impl LicenseLists {
    /// The client for `base_url` (already checked by `check_base_url`).
    ///
    /// # Errors
    ///
    /// [`AdapterError::BadConfig`] for an unusable base URL.
    pub fn new(base_url: &str) -> Result<Self, AdapterError> {
        let url = reqwest::Url::parse(base_url).map_err(|_| AdapterError::BadConfig)?;
        if url.scheme() == "file" {
            let dir = url.to_file_path().map_err(|_| AdapterError::BadConfig)?;
            return Ok(Self(Lists::Fixtures(dir)));
        }
        Ok(Self(Lists::Http(VendorHttp::new(base_url, Auth::None)?)))
    }

    async fn read(&self, segments: &[&str]) -> Result<Vec<u8>, AdapterError> {
        if !segments.iter().all(|name| is_safe_name(name)) {
            return Err(AdapterError::InvalidId);
        }
        match &self.0 {
            Lists::Fixtures(dir) => {
                let path: PathBuf = segments
                    .iter()
                    .fold(dir.clone(), |path, name| path.join(name));
                let read = tokio::task::spawn_blocking(move || std::fs::read(path)).await;
                let missing = AdapterError::Vendor {
                    status: 404,
                    code: None,
                };
                read.map_err(|_| AdapterError::OutcomeUnknown)?
                    .map_err(|_| missing)
            }
            Lists::Http(http) => http.send(http.request(Method::GET, segments, None)).await,
        }
    }
}

impl LicenseSource for LicenseLists {
    async fn fetch_daily_list(&self, list: &str) -> Result<Vec<u8>, AdapterError> {
        self.read(&[list]).await
    }

    async fn lookup_license(&self, number: &str) -> Result<Vec<u8>, AdapterError> {
        self.read(&["lookup", number]).await
    }
}
