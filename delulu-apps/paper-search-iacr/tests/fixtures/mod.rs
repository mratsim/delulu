//!  Delulu IACR Paper Search — Test Fixtures
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

//! # Test Fixtures
//!
//! Loads fixture files from `.zst`-compressed sources.

use std::path::PathBuf;

/// Returns the path to the fixtures directory.
fn fixtures_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("tests").join("fixtures")
}

/// Load a `.zst` fixture file, decompressing it on read.
fn load_zst(name: &str) -> String {
    let path = fixtures_dir().join(name);
    let compressed = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture at {:?}: {}", path, e));
    let decompressed = zstd::decode_all(compressed.as_slice())
        .expect("zstd decompression failed");
    String::from_utf8(decompressed)
        .unwrap_or_else(|e| panic!("Fixture {:?} is not valid UTF-8: {}", path, e))
}

/// Load the IACR RSS feed fixture XML.
pub fn iacr_rss_feed() -> String {
    load_zst("iacr-rss.xml.zst")
}

/// Load the IACR paper HTML fixture.
pub fn iacr_paper_html() -> String {
    load_zst("iacr-paper.html.zst")
}
