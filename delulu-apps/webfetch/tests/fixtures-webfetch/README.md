# Test Fixtures

## blog/dankrad.de/
- `pcs-multiproofs.html.zst` — Blog article: PCS multiproofs using random evaluation.
  Generic HTML (non-Discourse) page from dankrad's personal blog.
  Author: dankrad. Fetched: 2024-07-11.
- `dankrad-pcs-multiproofs-readability-0.6.0.html` — Readability.js v0.6.0 expected output.
- `dankrad-pcs-multiproofs-trafilatura-2.1.0.md` — Trafilatura v2.1.0 expected output.

## forum-discourse/ethresear.ch/
- `reed-solomon.html.zst` — Discourse topic #3039: Reed-Solomon erasure code recovery.
  HTML with Discourse meta generator tag and JSON-LD. 12 posts, OP by vbuterin.
  Fetched: 2024-07-11.
- `reed-solomon.json.zst` — JSON API response for the same topic.
- `ethresear-reed-solomon-readability-0.6.0.html` — Readability.js v0.6.0 expected output.
- `ethresear-reed-solomon-trafilatura-2.1.0.md` — Trafilatura v2.1.0 expected output.

## blog/gwern.net/
- `complement.html.zst` — Long-form essay: “Laws of Tech: Commoditize Your Complement”.
  Gwern's annotation-heavy personal site; section headings + footnotes + backlinks in the HTML.
  The backlink/similar-link contexts are JS-fetched at runtime and are NOT in the raw HTML
  (see TODO_fixtures.md — static extraction covers ~62% of the Firefox-rendered page).
  Fetched: 2026-08-06 (curl, Safari UA).
- `gwern-complement-readability-0.6.0.html` — Readability.js v0.6.0 expected output.
- `gwern-complement-trafilatura-2.1.0.md` — Trafilatura v2.1.0 expected output.

## blog/jaymartin.substack.com/
- `china-doesnt-need-to-win-the-ai-race.html.zst` — Substack article: “China Doesn't Need to Win the AI Race”.
  Server-rendered article body; comments/subscribe chrome present in the HTML.
  Fetched: 2026-08-06 (curl, Safari UA).
- `jaymartin-china-doesnt-need-to-win-the-ai-race-readability-0.6.0.html` — Readability.js v0.6.0 expected output.
- `jaymartin-china-doesnt-need-to-win-the-ai-race-trafilatura-2.1.0.md` — Trafilatura v2.1.0 expected output.

## blog/vllm.ai/
- `blog.html.zst` — Blog index page: https://vllm.ai/blog.
  A grid of ~40 article cards (title + date + read time -> /blog/YYYY-MM-DD-slug).
  Reference case for the LLM-navigation pipeline (TODO_custom_pipeline_ideas.md):
  article extractors strip these links by design; the navigation mode must
  keep them. Raw HTML only (407 KB) — expected outputs come with the custom
  pipeline. Fetched: 2026-08-06 (curl, Safari UA).
## wikipedia/
- `rust-programming-language.html.zst` — Wikipedia article: Rust (programming language).
  Large encyclopedia article (~1 MB HTML): infobox, code examples, references.
  Fetched: 2026-08-06 (curl, Safari UA).
- `rust-programming-language-readability-0.6.0.html` — Readability.js v0.6.0 expected output.
- `rust-programming-language-trafilatura-2.1.0.md` — Trafilatura v2.1.0 expected output.
## reddit/
- `reddit-thread-simple.json.zst` — Reddit API response (simple thread with nested replies).

## js-challenge/
- `google-enablejs.html.zst` — Faithful synthetic reproduction of the Google
  "enable JavaScript" interstitial: a `<script>` containing ≫200 chars of
  escaped JS (`\u003c`-style), near-zero visible text (total visible budget
  < 200 bytes), **marker-scrubbed** (no anti-bot/consent/paywall markers) and
  **JSON-LD-free**. No SPA-shell or enable-js marker is present, so it classifies
  `JSHeavy` **by script-dominance measurement alone** through the real
  `filter_trafilatura` pipeline (SC-2/SC-3).
