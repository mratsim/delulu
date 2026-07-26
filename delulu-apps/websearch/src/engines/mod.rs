//!  Delulu Web Search
//!
//!  Copyright (C) 2026  Mamy Ratsimbazafy
//!
//!  This program is free software: you can redistribute it and/or modify
//!  it under the terms of the GNU Affero General Public License as published by
//!  the Free Software Foundation, either version 3 of the License, or
//!  (at your option) any later version.
//!
//!  This program is distributed in the hope that it will be useful,
//!  but WITHOUT ANY WARRANTY; without even the implied warranty of
//!  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//!  GNU Affero General Public License for more details.
//!
//!  You should have received a copy of the GNU Affero General Public License
//!  along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Engine registry and backend modules.

pub mod duckduckgo;
pub mod brave;

use crate::engine::EngineRef;
use std::collections::HashMap;

/// Thread-safe registry of search engine backends.
///
/// Engines are stored as `Arc<dyn Engine + Send + Sync>` and keyed by
/// their string name (e.g., "duckduckgo", "brave").
///
/// Engines are registered once at startup and never modified afterward.
/// No lock needed — the HashMap is immutable after construction.
pub struct EngineRegistry {
    engines: HashMap<&'static str, EngineRef>,
}

impl EngineRegistry {
    /// Create a new empty engine registry.
    pub fn new() -> Self {
        Self {
            engines: HashMap::new(),
        }
    }

    /// Register an engine under the given name.
    pub fn register(&mut self, name: &'static str, engine: EngineRef) {
        self.engines.insert(name, engine);
    }

    /// Get an engine by name.
    ///
    /// Returns `None` if the engine name is not registered.
    pub fn get_engine(&self, name: &str) -> Option<EngineRef> {
        self.engines.get(name).cloned()
    }

    /// List all registered engine names.
    pub fn list_engines(&self) -> Vec<&'static str> {
        self.engines.keys().copied().collect()
    }
}

impl Default for EngineRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a registry with all available engines registered.
///
/// # Precondition
/// None.
///
/// # Postcondition
/// Returns a registry with DuckDuckGo and Brave engines registered.
pub fn create_default_registry() -> EngineRegistry {
    use std::sync::Arc;

    let mut registry = EngineRegistry::new();
    // DuckDuckGo — uses Safari TLS/HTTP2 fingerprint (Firefox gets blocked from this IP)
    let ddg_crawler = delulu_rate_limited_crawler::RateLimitedCrawler::builder()
        .with_emulation(wreq_util::Profile::Safari18_5)
        .with_qps(1)
        .with_max_resp_size(5 * 1024 * 1024)
        .with_timeout(std::time::Duration::from_secs(10))
        .with_http2()
        .build()
        .expect("Failed to build DuckDuckGo crawler");
    registry.register(
        "duckduckgo",
        Arc::new(duckduckgo::DuckDuckGoEngine::new(ddg_crawler)),
    );

    // Brave
    let brave_crawler = delulu_rate_limited_crawler::RateLimitedCrawler::builder()
        .with_qps(2)
        .with_max_resp_size(5 * 1024 * 1024)
        .with_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to build Brave crawler");
    registry.register(
        "brave",
        Arc::new(brave::BraveEngine::new(brave_crawler)),
    );

    registry
}

#[cfg(test)]
#[path = "../../tests/unit/engines/mod_test.rs"]
mod engines_test;
