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
