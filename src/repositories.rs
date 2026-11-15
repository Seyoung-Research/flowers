//! Repository integrations behind a connector trait (X5).
//!
//! External sample repositories (LabArchives, BioBook, FlowRepository, Cytobank,
//! BD Research Cloud, FlowJo Enterprise Server, FlowDx) are reached through the
//! [`RepositoryConnector`] trait so the analysis core never depends on a concrete
//! vendor protocol. The trait is the documented plugin boundary: concrete
//! adapters are implemented by callers (or future vendor-specific modules) and
//! registered with [`RepositoryHost`]. Credentials are passed per connection and
//! are never persisted inside workspace files.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced by repository connectors and hosts.
#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error("connection error: {0}")]
    Connection(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("authentication error: {0}")]
    Auth(String),
}

/// The external systems that sample repositories integrate with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepositoryKind {
    /// LabArchives.
    LabArchives,
    /// BioBook.
    BioBook,
    /// FlowRepository.
    FlowRepository,
    /// Cytobank.
    Cytobank,
    /// BD Research Cloud.
    BdResearchCloud,
    /// FlowJo Enterprise Server.
    FlowJoEnterpriseServer,
    /// FlowDx.
    FlowDx,
}

impl RepositoryKind {
    /// Every supported repository kind, in reporting order.
    pub fn all() -> &'static [RepositoryKind] {
        &[
            RepositoryKind::LabArchives,
            RepositoryKind::BioBook,
            RepositoryKind::FlowRepository,
            RepositoryKind::Cytobank,
            RepositoryKind::BdResearchCloud,
            RepositoryKind::FlowJoEnterpriseServer,
            RepositoryKind::FlowDx,
        ]
    }

    /// Stable lowercase identifier, e.g. `lab-archives`.
    pub fn as_str(&self) -> &'static str {
        match self {
            RepositoryKind::LabArchives => "lab-archives",
            RepositoryKind::BioBook => "bio-book",
            RepositoryKind::FlowRepository => "flow-repository",
            RepositoryKind::Cytobank => "cytobank",
            RepositoryKind::BdResearchCloud => "bd-research-cloud",
            RepositoryKind::FlowJoEnterpriseServer => "flowjo-enterprise-server",
            RepositoryKind::FlowDx => "flowdx",
        }
    }
}

impl fmt::Display for RepositoryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for RepositoryKind {
    type Err = RepositoryError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>();
        for kind in Self::all() {
            let kind_key = kind
                .as_str()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            if kind_key == normalized {
                return Ok(*kind);
            }
        }
        Err(RepositoryError::Unsupported(format!(
            "unknown repository: {s}"
        )))
    }
}

/// Connection parameters of a repository session (never persisted in workspaces).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionProfile {
    pub kind: RepositoryKind,
    pub endpoint: String,
    pub token: Option<String>,
}

/// Query used to list repository samples.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListQuery {
    /// Free-text sample-name / keyword filter.
    pub text: Option<String>,
    /// Maximum number of samples to return; `0` means unbounded.
    pub max_results: usize,
}

/// A sample record as reported by a repository listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositorySample {
    pub id: String,
    pub name: String,
    pub keywords: BTreeMap<String, String>,
    pub fcs_path: Option<String>,
}

/// Metadata describing a single repository sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryMetadata {
    pub id: String,
    pub name: String,
    pub keywords: BTreeMap<String, String>,
    pub size_bytes: Option<u64>,
}

/// Result of an upload attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryUploadReport {
    pub accepted: bool,
    pub reference: Option<String>,
}

/// Boundary every sample-repository integration must implement (X5).
///
/// Concrete adapters (LabArchives, BioBook, FlowRepository, Cytobank, BD Research
/// Cloud, FlowJo Enterprise Server, FlowDx) implement this trait; the analysis core
/// only sees [`RepositoryHost`] and this trait object.
pub trait RepositoryConnector: Send + Sync {
    /// The repository this connector talks to.
    fn kind(&self) -> RepositoryKind;

    /// Open a session against the repository.
    fn connect(&self, endpoint: &str, token: &str) -> Result<ConnectionProfile, RepositoryError>;

    /// List samples matching the query.
    fn list(&self, query: &ListQuery) -> Result<Vec<RepositorySample>, RepositoryError>;

    /// Download the FCS bytes of a sample.
    fn download(&self, sample_id: &str) -> Result<Vec<u8>, RepositoryError>;

    /// Upload an FCS file for a sample.
    fn upload(
        &self,
        sample: &RepositorySample,
        fcs_bytes: &[u8],
    ) -> Result<RepositoryUploadReport, RepositoryError>;

    /// Fetch metadata for a single sample.
    fn metadata(&self, sample_id: &str) -> Result<RepositoryMetadata, RepositoryError>;
}

/// Registry of repository connectors keyed by [`RepositoryKind`].
#[derive(Default)]
pub struct RepositoryHost {
    connectors: Vec<Box<dyn RepositoryConnector>>,
}

impl RepositoryHost {
    /// A fresh, empty host.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a connector; a connector of the same kind replaces the previous one.
    pub fn register(&mut self, connector: Box<dyn RepositoryConnector>) -> bool {
        let kind = connector.kind();
        if let Some(existing) = self.connectors.iter_mut().find(|c| c.kind() == kind) {
            *existing = connector;
            true
        } else {
            self.connectors.push(connector);
            true
        }
    }

    /// Remove the connector registered for `kind`.
    pub fn unregister(&mut self, kind: RepositoryKind) -> bool {
        let before = self.connectors.len();
        self.connectors.retain(|c| c.kind() != kind);
        self.connectors.len() != before
    }

    /// Resolve a connector by kind.
    pub fn resolve(&self, kind: RepositoryKind) -> Option<&dyn RepositoryConnector> {
        self.connectors
            .iter()
            .find(|c| c.kind() == kind)
            .map(|c| &**c)
    }

    /// All registered kinds, in registration order.
    pub fn kinds(&self) -> Vec<RepositoryKind> {
        self.connectors.iter().map(|c| c.kind()).collect()
    }

    /// Number of registered connectors.
    pub fn len(&self) -> usize {
        self.connectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.connectors.is_empty()
    }

    /// Remove every connector (reset).
    pub fn clear(&mut self) {
        self.connectors.clear();
    }
}

impl std::fmt::Debug for RepositoryHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepositoryHost")
            .field("kinds", &self.kinds())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn mock_connector(kind: RepositoryKind) -> Box<dyn RepositoryConnector> {
        Box::new(MockConnector {
            kind,
            samples: vec![
                RepositorySample {
                    id: "r1".to_string(),
                    name: "sample-a".to_string(),
                    keywords: [("FL1".to_string(), "FITC".to_string())]
                        .into_iter()
                        .collect(),
                    fcs_path: Some("/data/sample-a.fcs".to_string()),
                },
                RepositorySample {
                    id: "r2".to_string(),
                    name: "sample-b".to_string(),
                    keywords: Default::default(),
                    fcs_path: None,
                },
            ],
        })
    }

    struct MockConnector {
        kind: RepositoryKind,
        samples: Vec<RepositorySample>,
    }

    impl RepositoryConnector for MockConnector {
        fn kind(&self) -> RepositoryKind {
            self.kind
        }

        fn connect(
            &self,
            endpoint: &str,
            token: &str,
        ) -> Result<ConnectionProfile, RepositoryError> {
            if endpoint.is_empty() || token.is_empty() {
                return Err(RepositoryError::Connection(
                    "endpoint and token are required".to_string(),
                ));
            }
            Ok(ConnectionProfile {
                kind: self.kind,
                endpoint: endpoint.to_string(),
                token: Some(token.to_string()),
            })
        }

        fn list(&self, query: &ListQuery) -> Result<Vec<RepositorySample>, RepositoryError> {
            let mut samples: Vec<RepositorySample> = self
                .samples
                .iter()
                .filter(|s| {
                    query
                        .text
                        .as_deref()
                        .map(|text| {
                            s.name
                                .to_ascii_lowercase()
                                .contains(&text.to_ascii_lowercase())
                        })
                        .unwrap_or(true)
                })
                .cloned()
                .collect();
            if query.max_results > 0 && samples.len() > query.max_results {
                samples.truncate(query.max_results);
            }
            Ok(samples)
        }

        fn download(&self, sample_id: &str) -> Result<Vec<u8>, RepositoryError> {
            if sample_id == "r1" {
                Ok(b"FAX3.0-mock".to_vec())
            } else {
                Err(RepositoryError::NotFound(format!("sample {sample_id}")))
            }
        }

        fn upload(
            &self,
            sample: &RepositorySample,
            fcs_bytes: &[u8],
        ) -> Result<RepositoryUploadReport, RepositoryError> {
            if fcs_bytes.is_empty() {
                return Err(RepositoryError::Unsupported(
                    "empty upload payload".to_string(),
                ));
            }
            Ok(RepositoryUploadReport {
                accepted: true,
                reference: Some(sample.id.clone()),
            })
        }

        fn metadata(&self, sample_id: &str) -> Result<RepositoryMetadata, RepositoryError> {
            let sample = self
                .samples
                .iter()
                .find(|s| s.id == sample_id)
                .ok_or_else(|| RepositoryError::NotFound(sample_id.to_string()))?;
            Ok(RepositoryMetadata {
                id: sample.id.clone(),
                name: sample.name.clone(),
                keywords: sample.keywords.clone(),
                size_bytes: Some(1024),
            })
        }
    }

    #[test]
    fn repository_kind_known_names_parse() {
        assert_eq!(
            RepositoryKind::from_str("LabArchives").expect("lab archives"),
            RepositoryKind::LabArchives
        );
        assert_eq!(
            RepositoryKind::from_str("BIO-BOOK").expect("bio book"),
            RepositoryKind::BioBook
        );
        assert_eq!(
            RepositoryKind::from_str("flow-repository").expect("flow repository"),
            RepositoryKind::FlowRepository
        );
        assert_eq!(
            RepositoryKind::from_str("Cytobank").expect("cytobank"),
            RepositoryKind::Cytobank
        );
        assert_eq!(
            RepositoryKind::from_str("bd-research-cloud").expect("bd research cloud"),
            RepositoryKind::BdResearchCloud
        );
        assert_eq!(
            RepositoryKind::from_str("FlowJo Enterprise Server").expect("flowjo server"),
            RepositoryKind::FlowJoEnterpriseServer
        );
        assert_eq!(
            RepositoryKind::from_str("flowdx").expect("flowdx"),
            RepositoryKind::FlowDx
        );
    }

    #[test]
    fn repository_kind_unknown_name_is_unsupported() {
        let error = RepositoryKind::from_str("not-a-repository").expect_err("unknown kind");
        assert!(matches!(error, RepositoryError::Unsupported(_)));
        assert!(!RepositoryKind::all().is_empty());
    }

    #[test]
    fn mock_connector_connect_list_download_upload_metadata() {
        let connector = mock_connector(RepositoryKind::LabArchives);
        let profile = connector
            .connect("https://labarchives.example.com", "secret-token")
            .expect("connect");
        assert_eq!(profile.kind, RepositoryKind::LabArchives);
        assert_eq!(profile.endpoint, "https://labarchives.example.com");
        assert_eq!(profile.token.as_deref(), Some("secret-token"));
        assert!(connector.connect("", "").is_err());

        let mut host = RepositoryHost::new();
        host.register(connector);
        let resolved = host.resolve(RepositoryKind::LabArchives).expect("resolved");
        let listed = resolved.list(&ListQuery::default()).expect("list");
        assert_eq!(listed.len(), 2);

        let filtered = resolved
            .list(&ListQuery {
                text: Some("sample-b".to_string()),
                max_results: 0,
            })
            .expect("filtered list");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "sample-b");

        let capped = resolved
            .list(&ListQuery {
                text: None,
                max_results: 1,
            })
            .expect("capped list");
        assert_eq!(capped.len(), 1);

        let bytes = resolved.download("r1").expect("download");
        assert_eq!(bytes, b"FAX3.0-mock");
        assert!(resolved.download("missing").is_err());

        let report = resolved.upload(&listed[0], b"fcs-bytes").expect("upload");
        assert!(report.accepted);
        assert_eq!(report.reference.as_deref(), Some("r1"));

        let metadata = resolved.metadata("r1").expect("metadata");
        assert_eq!(metadata.id, "r1");
        assert_eq!(metadata.name, "sample-a");
        assert_eq!(metadata.size_bytes, Some(1024));
        assert!(resolved.metadata("missing").is_err());
    }

    #[test]
    fn repository_host_register_resolve_unregister() {
        let mut host = RepositoryHost::new();
        assert!(host.is_empty());
        assert_eq!(host.len(), 0);
        assert!(host.register(mock_connector(RepositoryKind::Cytobank)));
        assert_eq!(host.len(), 1);
        assert_eq!(host.kinds(), vec![RepositoryKind::Cytobank]);
        assert!(host.resolve(RepositoryKind::Cytobank).is_some());
        assert!(host.resolve(RepositoryKind::FlowDx).is_none());
        assert!(host.unregister(RepositoryKind::Cytobank));
        assert!(host.is_empty());
        assert!(!host.unregister(RepositoryKind::Cytobank));
    }
}
