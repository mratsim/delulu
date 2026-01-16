//!  Delulu Travel Agent
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

// Library for delulu-mcp-travel-agent
// MCP server for travel services (flights, hotels)

// Testing access - consent_cookie is re-exported for test modules
pub(crate) mod consent_cookie;
pub use consent_cookie::generate_cookie_header;
mod flights_proto;
mod flights_search;
mod hotels_query_builder;
mod hotels_results_parser;
mod hotels_search;

// Re-export commonly used items from flights_search
pub use flights_search::*;

// Re-export hotels_query_builder
pub use hotels_query_builder::{Amenity, HotelSearchParams, HotelSearchParamsBuilder, SortType};

// Re-export hotels_results_parser
pub use hotels_results_parser::{Hotel, HotelSearchResult};

// Re-export hotels_search
pub use hotels_search::{build_search_url, GoogleHotelsClient};
