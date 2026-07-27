#!/usr/bin/env python3
"""
Convert trafilatura eval HTML files into diagnostic fixtures.

Reads HTML files from ``_references_fetch/trafilatura/tests/eval/`` (and
``cache/`` as fallback) and annotations from
``_references_fetch/trafilatura/tests/evaldata.py``.

For each selected page:
  1. Compress HTML → ``source.html.zst``
  2. Run ``trafilatura.extract(html, output_format="markdown")`` → ``expected.md.zst``
  3. Write ``with[]`` / ``without[]`` → ``annotations.json``

Usage:
    python convert_eval_fixtures.py

Output:
    tests/fixtures-trafilatura/<slug>/
        source.html.zst
        expected.md.zst
        annotations.json
"""

import ast
import json
import os
import re
import sys

import trafilatura
import zstandard as zstd

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

# __file__ = <workspace>/delulu/delulu-apps/webfetch/tests/pipeline_trafilatura/convert_eval_fixtures.py
# We need the webfetch crate root (where Cargo.toml lives)
WEBFETCH_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
# That gives us <workspace>/delulu/delulu-apps/webfetch/

# Reference trafilatura eval data
# _references_fetch is at the workspace root, one level above the delulu repo
# <workspace>/delulu is the repo root
# <workspace>/_references_fetch is the reference data
DELULU_REPO = os.path.abspath(os.path.join(WEBFETCH_ROOT, "..", ".."))
# DELULU_REPO = <workspace>/delulu
WORKSPACE = os.path.abspath(os.path.join(DELULU_REPO, ".."))
# WORKSPACE = <workspace>

REFERENCES_DIR = os.path.join(WORKSPACE, "_references_fetch", "trafilatura", "tests")
EVAL_HTML_DIR = os.path.join(REFERENCES_DIR, "eval")
CACHE_HTML_DIR = os.path.join(REFERENCES_DIR, "cache")
EVALDATA_PATH = os.path.join(REFERENCES_DIR, "evaldata.py")

# Output fixture directory (relative to CARGO_MANIFEST_DIR)
FIXTURES_DIR = os.path.join(WEBFETCH_ROOT, "tests", "fixtures-trafilatura")


# ---------------------------------------------------------------------------
# Parse evaldata.py
# ---------------------------------------------------------------------------


def parse_evaldata(path):
    """Parse evaldata.py to extract page annotations.

    Returns a dict mapping filename -> {with: [...], without: [...], url: str}
    """
    with open(path, "r", encoding="utf-8") as f:
        content = f.read()

    # Strip Python comments (naive — good enough for this data file)
    lines = content.split("\n")
    cleaned_lines = []
    for line in lines:
        in_string = False
        string_char = None
        for i, ch in enumerate(line):
            if in_string:
                if ch == string_char and (i == 0 or line[i - 1] != "\\"):
                    in_string = False
            else:
                if ch in ("'", '"'):
                    in_string = True
                    string_char = ch
                elif ch == "#":
                    line = line[:i]
                    break
        cleaned_lines.append(line)

    cleaned = "\n".join(cleaned_lines)

    # Find the EVAL_PAGES dict
    start = cleaned.find("EVAL_PAGES = {")
    if start == -1:
        print("ERROR: Could not find EVAL_PAGES in evaldata.py", file=sys.stderr)
        sys.exit(1)

    dict_str = cleaned[start + len("EVAL_PAGES = ") :]
    try:
        eval_pages = ast.literal_eval(dict_str)
    except Exception as e:
        print(f"ERROR: Failed to parse EVAL_PAGES: {e}", file=sys.stderr)
        sys.exit(1)

    pages = {}
    for url, info in eval_pages.items():
        filename = info.get("file", "")
        if filename:
            pages[filename] = {
                "with": info.get("with", []),
                "without": info.get("without", []),
                "url": url,
            }

    return pages


# ---------------------------------------------------------------------------
# HTML file resolution
# ---------------------------------------------------------------------------


def find_html_file(filename):
    """Find an HTML file in eval/ or cache/ directory."""
    path = os.path.join(EVAL_HTML_DIR, filename)
    if os.path.exists(path):
        return path
    path = os.path.join(CACHE_HTML_DIR, filename)
    if os.path.exists(path):
        return path
    return None


# ---------------------------------------------------------------------------
# Slug generation
# ---------------------------------------------------------------------------


def make_slug(url, filename):
    """Convert a URL and filename to a fixture directory slug.

    Derives the slug from the URL's domain and path, falling back to
    filename-based slug generation for non-hash filenames.

    Examples:
        https://www.adac.de/kindersitze/... -> adac-de-kindersitze
        https://en.wikipedia.org/wiki/TSNE -> en-wikipedia-org-tsne
        https://www.watson.ch/leben/drinks/sazerac -> watson-ch-sazerac
    """
    # Extract domain from URL
    if "//" in url:
        domain = url.split("/")[2]
    else:
        domain = ""
    if domain.startswith("www."):
        domain = domain[4:]

    # Check if filename is a hex hash (32+ hex chars)
    name_without_ext = filename.rsplit(".", 1)[0] if filename.endswith(".html") else filename
    is_hash = bool(re.match(r'^[a-fA-F0-9]{32,}$', name_without_ext))

    if is_hash:
        # Derive slug from URL: domain + last meaningful path segment
        path_parts = url.rstrip("/").split("/")
        descriptor = ""
        for part in reversed(path_parts):
            if part and not re.match(r'^\d+$', part):
                descriptor = part.lower()
                break
        # Clean descriptor
        descriptor = re.sub(r"[^a-zA-Z0-9-]", "", descriptor)
        descriptor = re.sub(r"-{2,}", "-", descriptor)
        descriptor = descriptor.strip("-")

        domain_slug = re.sub(r"[._]+", "-", domain.lower())
        domain_slug = re.sub(r"[^a-zA-Z0-9-]", "", domain_slug)

        if descriptor and descriptor != domain_slug:
            slug = f"{domain_slug}-{descriptor}"
        else:
            slug = domain_slug
    else:
        # Existing logic: derive from filename
        name = name_without_ext
        name = re.sub(r"[._]+", "-", name)
        name = re.sub(r"[^a-zA-Z0-9-]", "", name)
        name = re.sub(r"-{2,}", "-", name)
        name = name.strip("-")
        slug = name.lower()

    # Collapse multiple hyphens and strip
    slug = re.sub(r"-{2,}", "-", slug)
    slug = slug.strip("-")
    return slug


# ---------------------------------------------------------------------------
# Fixture selection
# ---------------------------------------------------------------------------


def select_fixtures(pages, count=15):
    """Select a diverse set of fixtures from available pages.

    Criteria: mix of sites, good annotation coverage, diverse domains.
    """
    # Only consider pages whose HTML file exists
    candidates = []
    for filename, info in pages.items():
        html_path = find_html_file(filename)
        if html_path is None:
            continue

        with_count = len(info.get("with", []))
        without_count = len(info.get("without", []))
        if with_count >= 2 and without_count >= 2:
            candidates.append((filename, info, with_count + without_count))

    # Sort by total annotation count (more annotations = more useful)
    candidates.sort(key=lambda x: x[2], reverse=True)

    # Pick top N with domain diversity
    selected = []
    seen_domains = set()
    for filename, info, _ in candidates:
        if len(selected) >= count:
            break
        # Extract domain for diversity
        url = info.get("url", "")
        domain = url.split("/")[2] if "//" in url else ""
        domain_base = ".".join(domain.split(".")[-2:]) if domain else ""

        if domain_base in seen_domains:
            continue

        selected.append(filename)
        seen_domains.add(domain_base)

    # If still not enough, add more regardless of domain
    if len(selected) < count:
        for filename, info, _ in candidates:
            if len(selected) >= count:
                break
            if filename not in selected:
                selected.append(filename)

    return selected


# ---------------------------------------------------------------------------
# Fixture creation
# ---------------------------------------------------------------------------


def create_fixture(filename, info, force=False):
    """Create a single fixture directory."""
    url = info.get("url", "")
    slug = make_slug(url, filename)
    fixture_dir = os.path.join(FIXTURES_DIR, slug)

    if os.path.exists(fixture_dir):
        if not force:
            print(f"  SKIP {slug} (already exists)")
            return True
    else:
        os.makedirs(fixture_dir, exist_ok=True)

    # Find source HTML
    html_path = find_html_file(filename)
    if html_path is None:
        print(f"  ERROR: {filename} not found", file=sys.stderr)
        return False

    with open(html_path, "r", encoding="utf-8", errors="replace") as f:
        html_content = f.read()

    if len(html_content) == 0:
        print(f"  ERROR: {filename} is empty", file=sys.stderr)
        return False

    # Compress source HTML
    cctx = zstd.ZstdCompressor(level=3)
    source_compressed = cctx.compress(html_content.encode("utf-8"))
    with open(os.path.join(fixture_dir, "source.html.zst"), "wb") as f:
        f.write(source_compressed)

    # Run trafilatura extraction
    try:
        expected_md = trafilatura.extract(
            html_content,
            output_format="markdown",
            include_comments=False,
            include_tables=True,
            no_fallback=False,
        )
        if expected_md is None:
            print(f"  WARN: {slug} - trafilatura returned None, using empty string")
            expected_md = ""
    except Exception as e:
        print(f"  ERROR: {slug} - trafilatura extraction failed: {e}", file=sys.stderr)
        expected_md = ""

    # Compress expected markdown
    expected_compressed = cctx.compress(expected_md.encode("utf-8"))
    with open(os.path.join(fixture_dir, "expected.md.zst"), "wb") as f:
        f.write(expected_compressed)

    # Write annotations
    annotations = {
        "with": info.get("with", []),
        "without": info.get("without", []),
    }
    with open(os.path.join(fixture_dir, "annotations.json"), "w", encoding="utf-8") as f:
        json.dump(annotations, f, ensure_ascii=False, indent=2)

    print(f"  OK   {slug} ({len(html_content)}B HTML, {len(expected_md)}B MD, "
          f"{len(annotations['with'])} with, {len(annotations['without'])} without)")
    return True


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main():
    print("=" * 60)
    print("  Trafilatura Eval Fixture Converter")
    print("=" * 60)

    # Validate paths
    for d, name in [(EVAL_HTML_DIR, "eval"), (CACHE_HTML_DIR, "cache")]:
        if os.path.isdir(d):
            html_count = len([f for f in os.listdir(d) if f.endswith(".html")])
            print(f"  {name}/: {html_count} HTML files ({d})")

    if not os.path.isfile(EVALDATA_PATH):
        print(f"ERROR: evaldata.py not found: {EVALDATA_PATH}", file=sys.stderr)
        sys.exit(1)

    print(f"  Output: {FIXTURES_DIR}")

    # Parse annotations
    print("\nParsing evaldata.py...")
    pages = parse_evaldata(EVALDATA_PATH)
    print(f"  Found {len(pages)} pages with annotations")

    # Count how many have HTML files
    available = sum(1 for f in pages if find_html_file(f) is not None)
    print(f"  {available} have HTML files available")

    # Select fixtures
    print("\nSelecting fixtures...")
    selected = select_fixtures(pages, count=15)
    print(f"  Selected {len(selected)} fixtures for conversion")

    # Create fixtures
    print("\nCreating fixtures...")
    os.makedirs(FIXTURES_DIR, exist_ok=True)

    success = 0
    failed = 0
    for filename in selected:
        info = pages.get(filename, {"with": [], "without": []})
        if create_fixture(filename, info, force=False):
            success += 1
        else:
            failed += 1

    # Summary
    print()
    print("=" * 60)
    print(f"  Summary: {success} created, {failed} failed")
    print(f"  Fixtures in: {FIXTURES_DIR}")
    print("=" * 60)


if __name__ == "__main__":
    main()
