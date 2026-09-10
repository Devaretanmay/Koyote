use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EdgeKind {
    Exposes,
    Declares,
    Wraps,
    Invokes,
    Repairs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from_id: String,
    pub to_id: String,
    pub kind: EdgeKind,
    #[serde(default)]
    pub metadata: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderNode {
    pub id: String,
    pub name: String,
    pub base_urls: Vec<String>,
    pub sdk_packages: Vec<String>,
    pub method_patterns: Vec<String>,
    pub latest_version: String,
    pub deprecation_deadline: String,
    pub migration_guide_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionNode {
    pub id: String,
    pub provider_id: String,
    pub version: String,
    pub release_date: String,
    pub deprecation_deadline: String,
    pub is_deprecated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractNode {
    pub id: String,
    pub provider_id: String,
    pub version_id: String,
    pub method_pattern: String,
    pub http_method: String,
    pub path: String,
    pub required_params: Vec<String>,
    pub is_breaking_change: bool,
    pub change_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestDepNode {
    pub id: String,
    pub manifest_path: String,
    pub package_name: String,
    pub declared_version: String,
    pub resolved_version: String,
    pub package_manager: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrapperNode {
    pub id: String,
    pub file_path: String,
    pub exported_symbol: String,
    pub wraps_package: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallsiteNode {
    pub id: String,
    pub file_path: String,
    pub line_number: usize,
    pub column: usize,
    pub line_content: String,
    pub matched_pattern: String,
    pub is_quarantined: bool,
    pub target_contract_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationNode {
    pub id: String,
    pub provider_name: String,
    pub from_version: String,
    pub to_version: String,
    pub description: String,
    pub affected_callsites: Vec<String>,
    pub is_merge_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExternalDependencyGraph {
    pub providers: Vec<ProviderNode>,
    pub versions: Vec<VersionNode>,
    pub contracts: Vec<ContractNode>,
    pub manifest_deps: Vec<ManifestDepNode>,
    pub wrappers: Vec<WrapperNode>,
    pub callsites: Vec<CallsiteNode>,
    pub migrations: Vec<MigrationNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtRiskItem {
    pub provider_name: String,
    pub package_name: String,
    pub current_version: String,
    pub target_version: String,
    pub breaking_change: String,
    pub callsites_count: usize,
    pub affected_files: Vec<String>,
    pub is_auto_repairable: bool,
    pub migration_guide_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistItem {
    pub provider_name: String,
    pub method_pattern: String,
    pub deprecation_deadline: String,
    pub days_remaining: Option<i64>,
    pub callsite_count: usize,
    pub documentation_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthyItem {
    pub provider_name: String,
    pub package_name: String,
    pub current_version: String,
    pub callsite_count: usize,
    pub status_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyAuditSummary {
    pub total_providers_detected: usize,
    pub total_callsites_mapped: usize,
    pub at_risk: Vec<AtRiskItem>,
    pub watchlist: Vec<WatchlistItem>,
    pub healthy: Vec<HealthyItem>,
    pub total_auto_repairable: usize,
}
