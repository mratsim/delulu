# TODO — webfetch extraction fixtures (real-world pages)

**Status:** Open — all three pages currently produce LOSSY extraction (evidence below).
**Owner:** webfetch pipeline (`filter_mozilla_readability` / generic-HTML extraction path).
**Goal:** these three pages must extract completely (article body, section headings, no junk) and be locked in as fixture-backed regression tests.

---

## The three target pages

| # | Page | URL | Why it matters | Status (measured 2026-08-06) |
|---|---|---|---|---|
| 1 | Rust (programming language) — Wikipedia | `https://en.wikipedia.org/wiki/Rust_(programming_language)` | Large real-world encyclopedia article; the canonical "big complex page" case | ❌ **Content loss** — 18 KB markdown, but "Cargo" and "1.0" (major sections) absent; nav junk at top |
| 2 | Laws of Tech: Commoditize Your Complement — Gwern | `https://gwern.net/complement` | Long-form essay with dense section structure, footnotes, annotations | ❌ **Heading loss + partial body** — 4,245 words extracted vs ~8,585 words visible in the raw HTML; **0 headings** in markdown while the HTML has section headings (`<h2 id=...>`) |
| 3 | China Doesn't Need to Win the AI Race — Jay Martin (Substack) | `https://jaymartin.substack.com/p/china-doesnt-need-to-win-the-ai-race` | Substack article; server-rendered body present in HTML | ❌ **Major body loss** — ~3,327 words visible in the served HTML, webfetch returns ~200 words (title + whitespace) |

## Measured evidence (2026-08-06, release binary `delulu-all-mcp` stdio)

### 1. Rust Wikipedia
- `webfetch` → 18,402 chars markdown; probes: `"memory safety"` ✓, `"History"` ✓, **`"Cargo"` ✗**, **`"1.0"` ✗**.
- The Cargo section and release-history content are dropped; output begins with whitespace/nav debris.
- Same failure family as pre-existing test `test_fetch_and_extract_generic_html_from_fixture` (readability drops a later paragraph) and `pipelines::mozilla_readability::tests::test_extraction_regression` (h1 dropped).

### 2. Gwern complement
- `webfetch` → 4,245 words, **0 `##`/`###` headings**, ends with annotation/footer junk (`similar links by topic`).
- `webfetch_raw` → 30,409 chars (also no essay structure).
- Raw HTML (curl, 204,894 bytes): visible text ~8,585 words; section headings present in markup (`<h2><a href="#information-rules" ...`).

### 3. Substack article
- `webfetch` → 211 words (frontmatter + title + whitespace).
- `webfetch_raw` → 9,259 chars, `GenericHtml` body effectively empty.
- Raw HTML (curl, 199,871 bytes): visible text ~3,327 words **including the full article body** ("It Was Never About the Houses Everyone remembers 2008..."). The body is server-rendered — a plain fetcher can see it; the pipeline drops it.
- Not a JS-hydration problem: curl sees the text; the extraction pipeline loses it.

## Root-cause hypothesis

- The **readability filter / generic-HTML fallback** in `delulu-apps/webfetch/src/pipelines/` loses content on pages with large/deep DOMs: headings dropped (Rust, Gwern), paragraphs/sections dropped (Rust, Substack).
- `MAX_NODES` is **not** the cause — it is an unimplemented TODO (`dom_convert.rs:52`); `MAX_DEPTH` only flattens nesting.
- Known pre-existing test failures in the same family: `mozilla_readability::tests::test_extraction_regression` (h1 lost) and `t_webfetch_library::test_fetch_and_extract_generic_html_from_fixture` (paragraph lost).
- Likely fix directions: investigate `filter_mozilla_readability` content scoring/truncation for large DOMs; verify the generic-HTML fallback path (which re-parses and may be where structure vanishes); add diagnostics comparing extracted vs raw visible-text size.

## Acceptance criteria ("properly handled")

For each fixture, `webfetch <url>` must produce markdown that:

1. **Rust Wikipedia**
   - Contains the lead + the article's major sections: "History", "Cargo", "1.0" (release history) present.
   - Section headings present as `##`-style markdown headings.
   - No leading nav/junk block.
2. **Gwern complement**
   - ≥ ~90% of the essay's visible words (baseline: raw HTML visible text ~8,585 words, minus nav/footnote chrome — target ≈ full essay).
   - Section headings present (the HTML has them).
   - No trailing annotation/footer junk.
3. **Substack article**
   - The full article body present (baseline ~3,327 visible words in HTML; target ≥ ~2,500 words of article content).
   - Title/author/date metadata in frontmatter; no "Subscribe/Sign in" junk.

## Fixture plan (follow existing convention)

Existing convention: `delulu-apps/webfetch/tests/fixtures-webfetch/<name>/source.html.zst` + `expected.md.zst` (zstd-compressed), consumed by snapshot/regression tests.

1. Capture each page's raw HTML (curl with a Safari UA — do NOT rely on the crawler's emulation for the stored fixture) → `tests/fixtures-webfetch/{rust-wikipedia,gwern-complement,substack-china-ai-race}/source.html.zst`.
2. Generate the EXPECTED markdown by hand-fixing the current lossy output (complete article, headings restored) → `expected.md.zst` — this encodes the acceptance criteria.
3. Add snapshot tests: `webfetch(source.html.zst)` output must equal `expected.md.zst` (mirror the existing `test_extraction_regression` / arxiv fixture test pattern in `tests/unit/lib_test.rs` and/or `tests/t_webfetch_library.rs`).
4. Fix the pipeline until the new tests pass **without** weakening the assertions.

## Verification procedure (manual, until fixtures land)

```bash
# binary already built: delulu/target/release/delulu-all-mcp
# drive stdio tools/call via the MCP protocol (see TESTRUN_READINESS.md pattern)
# for each URL:
#   tools/call webfetch      -> assess body/headings/junk vs acceptance criteria
#   tools/call webfetch_raw  -> fidelity comparison
# baseline: curl -sL -A "<Safari UA>" <url> -> strip tags -> visible word count
```

## Notes

- This is a **quality** tracker, not a correctness blocker for the personal test run: `webfetch_raw` is the current fidelity workaround; the fixtures lock the fix.
- The three URLs were chosen by the user as representative real-world pages (encyclopedia, long-form essay, Substack article).
- Do not close this TODO until all three snapshot tests exist and pass on a clean pipeline.
