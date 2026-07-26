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

//! Unit tests for engine registry.

use super::{create_default_registry, duckduckgo::DuckDuckGoEngine, EngineRegistry};
use delulu_rate_limited_crawler::RateLimitedCrawler;
use std::sync::Arc;

#[test]
fn registry_empty_on_new() {
    let mut registry = EngineRegistry::new();
    assert!(registry.list_engines().is_empty());
}

#[test]
fn registry_get_nonexistent() {
    let mut registry = EngineRegistry::new();
    assert!(registry.get_engine("nonexistent").is_none());
}

#[test]
fn registry_register_and_get() {
    let mut registry = EngineRegistry::new();
    let ddg_crawler = RateLimitedCrawler::builder()
        .with_qps(1)
        .with_max_resp_size(5 * 1024 * 1024)
        .with_timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    registry.register(
        "duckduckgo",
        Arc::new(DuckDuckGoEngine::new(ddg_crawler)),
    );
    let engine = registry.get_engine("duckduckgo");
    assert!(engine.is_some());
    let names = registry.list_engines();
    assert!(names.contains(&"duckduckgo"));
}

#[test]
fn create_default_registration() {
    let registry = create_default_registry();
    let names = registry.list_engines();
    assert!(names.contains(&"duckduckgo"));
    assert!(names.contains(&"brave"));
}
