//! Shared use cases and v1 response contracts for HTTP and CLI entry adapters.

use chemifree_domain::Capability;
use serde::Serialize;

pub const API_VERSION: &str = "v1";

/// An application-owned port implemented by infrastructure providers.
pub trait CapabilityCatalog: Send + Sync {
    fn capabilities(&self) -> Vec<Capability>;
}

pub struct Application<P> {
    catalog: P,
}

impl<P: CapabilityCatalog> Application<P> {
    pub fn new(catalog: P) -> Self {
        Self { catalog }
    }

    /// Liveness only. This does not assert that a scientific engine is available.
    pub fn health(&self) -> HealthResponse {
        HealthResponse {
            api_version: API_VERSION,
            service: "chemifree",
            version: env!("CARGO_PKG_VERSION"),
            stage: "scaffold",
            status: "ok",
        }
    }

    pub fn capabilities(&self) -> CapabilitiesResponse {
        CapabilitiesResponse {
            api_version: API_VERSION,
            capabilities: self
                .catalog
                .capabilities()
                .into_iter()
                .map(|capability| CapabilityResponse {
                    id: capability.id.as_str(),
                    label: capability.label,
                    status: capability.status.as_str(),
                    description: capability.description,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub api_version: &'static str,
    pub service: &'static str,
    pub version: &'static str,
    pub stage: &'static str,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct CapabilitiesResponse {
    pub api_version: &'static str,
    pub capabilities: Vec<CapabilityResponse>,
}

#[derive(Debug, Serialize)]
pub struct CapabilityResponse {
    pub id: &'static str,
    pub label: &'static str,
    pub status: &'static str,
    pub description: &'static str,
}
