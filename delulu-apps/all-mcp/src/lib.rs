//!  Delulu All-MCP — Library
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
//!
//!
//! # Delulu All-MCP
//!
//! Unified MCP server exposing the 21-tool union across webfetch, websearch,
//! travel, arxiv, iacr, and pubmed. The MCP types live in `lib_mcp` (feature
//! `mcp`) so both the standalone `delulu-all-mcp` binary and the reusable
//! library share them. The `AllServer` itself ships in a later phase.

#[cfg(feature = "mcp")]
pub mod lib_mcp;

#[cfg(feature = "mcp")]
pub use lib_mcp::{AllMcpConfig, ServerId, TOOL_ROUTES};