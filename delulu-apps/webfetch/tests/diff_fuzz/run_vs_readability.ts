// Run: cd tests/diff_fuzz && cargo build --release -p delulu-webfetch && node --experimental-strip-types run_vs_readability.ts
// Requires: Node.js >= 22.6.0, npm install in tests/diff_fuzz/
import { Readability } from "@mozilla/readability";
import { JSDOM } from "jsdom";
import { execFileSync } from "node:child_process";
import * as path from "node:path";

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const HERE = import.meta.dirname;
const FIXTURE_DIR = path.resolve(HERE, "..", "fixtures-webfetch");
const CLI_BINARY = path.resolve(
  HERE, "..", "..", "..", "..", "target", "release", "delulu-fetch"
);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface DiffResult {
  fixture: string;
  rust_ok: boolean;
  js_ok: boolean;
  rust_headings: string[];
  js_headings: string[];
  headings_match: boolean;
  rust_para_count: number;
  js_para_count: number;
  para_count_match: boolean;
  /** Distance: |rust_headings.length - js_headings.length| + |rust_para_count - js_para_count|.
   *  0 = perfect structural match. Higher = more structural divergence. */
  structure_distance: number;
  /** Float in [0.0, 1.0]: word-level set-difference similarity of article text
   * extracted from <p>, <pre>, <td>, <li>, <blockquote> tags.
   *  1.0 = perfect word overlap, 0.0 = no shared words.
   *  Formula: 1 - (|A\B| + |B\A|) / (|A| + |B|) on unique lowercase word sets.
   *  Returns 0.0 when both documents yield no words (BOTH_EMPTY guard overrides 1.0 to 0.0).
   *  Returns 0.0 when one document yields words but the other does not.
   *  Set to 0.0 when either runner fails to produce output.
  content_score: number;
  error?: string;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function extractHeadings(text: string): string[] {
  const md = text.match(/^(#{1,6})\s+(.+)$/gm);
  if (md) return md.map((l) => l.replace(/^#+\s+/, "").trim());
  const html = [...text.matchAll(/<h([1-6])[^>]*>(.+?)<\/h\1>/gs)];
  if (html.length > 0) return html.map((m) => m[2].replace(/<[^>]+>/g, "").trim());
  return [];
}

function countParagraphs(text: string): number {
  const blocks = text.split(/\n\n+/).map((b) => b.trim()).filter(Boolean);
  const paras = blocks.filter(
    (b) => !b.startsWith("#") && !b.startsWith("---") && !b.startsWith("|")
  );
  if (paras.length > 0) return paras.length;
  const htmlParas = [...text.matchAll(/<p[^>]*>(.+?)<\/p>/gs)];
  return htmlParas.length || 0;
}


/**
 * Strip ALL HTML tags from the input, returning raw text content.
 * Uses simple regex tag removal (not a full HTML parser).
 *
 * Known limitations:
 * - Malformed nested angle brackets may cause performance degradation
 * - Script/style/SVG content is preserved as text (only tag delimiters <> are removed)
 * - Does NOT decode HTML entities
 *
 * @param html - HTML string to strip. Must be a string (throws TypeError otherwise).
 * @returns Raw text with all tags removed, whitespace normalized, trimmed.
 */
function stripAllHtml(html: string): string {
  return html.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
}

/**
 * Compute word-level multiset edit distance between two HTML/text documents.
 *
 * Strips all HTML tags from both inputs, tokenizes into lowercase words,
 * and computes: 1 - (missingRefWords + extraOutputWords) / refWordCount
 * using word-frequency maps (multiset), NOT Set-based dedup.
 *
 * Formula asymmetry (known trade-off): Extra output words are penalized as
 * harshly as missing reference words. A verbose output containing all reference
 * words plus extras may score 0.0.
 *
 * Pre-condition: Both inputs are HTML strings (stripAllHtml is called internally).
 * Post-condition: Return value in [0.0, 1.0], clamped.
 * Throws: TypeError if either argument is not a string.
 * Known limitation: /\W+/ tokenization is unsuitable for CJK/Unicode text.
 *
 * @param rustHtml - HTML output from Rust pipeline
 * @param referenceInput - Reference HTML or plain text
 * @returns Content similarity score in [0.0, 1.0]
 */
function contentSimilarity(rustHtml: string, referenceInput: string): number {
  const rustText = stripAllHtml(rustHtml);
  const refText = stripAllHtml(referenceInput);

  const refWords = refText.toLowerCase().split(/\W+/).filter(Boolean);
  const outWords = rustText.toLowerCase().split(/\W+/).filter(Boolean);

  if (refWords.length === 0) {
    return outWords.length === 0 ? 1.0 : 0.0;
  }

  // Frequency maps (multiset) — NOT Set
  const refFreq = new Map<string, number>();
  for (const w of refWords) refFreq.set(w, (refFreq.get(w) || 0) + 1);

  const outFreq = new Map<string, number>();
  for (const w of outWords) outFreq.set(w, (outFreq.get(w) || 0) + 1);

  // Missing words from reference (in ref but not in output, or undercounted)
  let missingRefWords = 0;
  for (const [word, refCount] of refFreq) {
    const outCount = outFreq.get(word) || 0;
    if (refCount > outCount) missingRefWords += refCount - outCount;
  }

  // Extra words in output (not in ref, or overcounted)
  let extraOutputWords = 0;
  for (const [word, outCount] of outFreq) {
    const refCount = refFreq.get(word) || 0;
    if (outCount > refCount) extraOutputWords += outCount - refCount;
  }

  const refWordCount = refWords.length; // total word count (NOT unique set size)
  const score = 1.0 - (missingRefWords + extraOutputWords) / refWordCount;
  return Math.max(0, Math.min(1, score));
}


function decompressZst(p: string): string {
  return execFileSync("unzstd", ["--stdout", p], {
    encoding: "utf-8",
    maxBuffer: 10 * 1024 * 1024,
    timeout: 30_000,
    stdio: ["pipe", "pipe", "ignore"],
  });
}

function runReadabilityJS(html: string): { title: string; content: string } | null {
  const dom = new JSDOM(html);
  const reader = new Readability(dom.window.document);
  return reader.parse();
}

function runDeluluFetch(html: string): string {
  return execFileSync(CLI_BINARY, ["-i", "-", "--output-format", "html"], {
    input: html,
    encoding: "utf-8",
    timeout: 30_000,
    stdio: ["pipe", "pipe", "ignore"],
  });
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  const fixtures = [
    "dankrad-pcs-multiproofs.html.zst",
    "ethresear-reed-solomon.html.zst",
  ];

  const results: DiffResult[] = [];

  for (const fixture of fixtures) {
    const fixturePath = path.join(FIXTURE_DIR, fixture);

    // Decompress
    let html: string;
    try {
      html = decompressZst(fixturePath);
    } catch (e: any) {
      const code = e.code;
      if (code === 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER') {
        console.error(`FATAL: decompress output exceeded maxBuffer (${e.message})`);
        process.exit(1);
      } else if (code === 'ENOENT') {
        console.error(`FATAL: unzstd not found in PATH — is zstd installed? (${e.message})`);
        process.exit(1);
      } else {
        console.error(`FATAL: decompress failed: ${e.message}`);
        process.exit(1);
      }
        console.log(`DECOMPRESS_FAIL: ${e.message}`);
      }
      results.push({
        fixture,
        rust_ok: false,
        js_ok: false,
        rust_headings: [],
        js_headings: [],
        headings_match: true,
        rust_para_count: 0,
        js_para_count: 0,
        para_count_match: true,
        structure_distance: 0,
        content_score: 0.0,
        error: `decompress failed: ${e.message}`,
      });
      continue;
    }

    // Run JS Readability
    let jsOutput = "";
    let jsOk = false;
    let jsError: string | undefined;
    try {
      const article = runReadabilityJS(html);
      if (article && article.content) {
        jsOutput = article.content;
        jsOk = true;
      }
    } catch (e: any) {
      jsError = e?.message ?? String(e);
    }

    // Run Rust delulu-fetch
    let rustOutput = "";
    let rustOk = false;
    let rustError: string | undefined;
    try {
      rustOutput = runDeluluFetch(html);
      rustOk = true;
    } catch (e: any) {
      // Fatal precondition: CLI binary not found
      if (e.code === 'ENOENT') {
        throw new Error(
          `CLI binary not found at ${CLI_BINARY}: ${e.message}`
        );
      }
      rustError = e?.message ?? String(e);
    }

    // Compare headings
    const rustHeadings = extractHeadings(rustOutput);
    const jsHeadings = extractHeadings(jsOutput);
    const headingsMatch =
      JSON.stringify(rustHeadings) === JSON.stringify(jsHeadings);

    // Compare paragraph counts
    const rustParaCount = countParagraphs(rustOutput);
    const jsParaCount = countParagraphs(jsOutput);
    const paraCountMatch = rustParaCount === jsParaCount;

    // Content similarity
    let contentScore = contentSimilarity(rustOutput, jsOutput);

    const structureDist = Math.abs(rustHeadings.length - jsHeadings.length)
      + Math.abs(rustParaCount - jsParaCount);

    // BOTH_EMPTY guard: if both outputs are empty, score as 0.0 (not 1.0)
    const bothEmpty = rustOutput === "" && jsOutput === "";
    if (bothEmpty) {
      contentScore = 0.0;
    }

    const result: DiffResult = {
      fixture,
      rust_ok: rustOk,
      js_ok: jsOk,
      rust_headings: rustHeadings,
      js_headings: jsHeadings,
      headings_match: headingsMatch,
      rust_para_count: rustParaCount,
      js_para_count: jsParaCount,
      para_count_match: paraCountMatch,
      structure_distance: structureDist,
      content_score: contentScore,
    };

    if (jsError) result.error = `JS Readability failed: ${jsError}`;
    if (rustError) result.error = `Rust delulu-fetch failed: ${rustError}`;
    if (bothEmpty && jsOk && rustOk && !result.error) {
      result.error = "BOTH_EMPTY";
    }

    // Output JSON line to stdout
    console.log(JSON.stringify(result));

    results.push(result);
  }

  // --- Summary ---
  console.log("\n─── Summary ───\n");
  let passed = 0;
  let total = results.length;
  for (const r of results) {
    const ok = r.rust_ok && r.js_ok && r.structure_distance <= 10 && r.content_score > 0.9;
    if (ok) passed++;
    const icon = ok ? "✓" : "✗";
    console.log(
      `  ${icon} ${r.fixture}: ` +
        `struct=${r.structure_distance} ` +
        `cont=${r.content_score.toFixed(3)} ` +
        `headings=${r.headings_match} ` +
        `paras=${r.para_count_match} ` +
        (r.error ? ` error=${r.error}` : "")
    );
  }
  console.log(`\n  ${passed}/${total} passed (struct + content + execution)`);

  // Show failure details
  const failures = results.filter((r) => !r.rust_ok || !r.js_ok || r.structure_distance > 10 || r.content_score <= 0.9);
  if (failures.length > 0) {
    console.log("\n─── Failures ───\n");
    for (const f of failures) {
      console.log(`  Fixture: ${f.fixture}`);
      if (f.error) console.log(`    Error: ${f.error}`);
      if (!f.rust_ok) console.log(`    Rust execution failed`);
      if (!f.js_ok) console.log(`    JS execution failed`);
      if (f.structure_distance > 10) {
        console.log(`    Structure: distance (${f.structure_distance})`);
        if (!f.headings_match) {
          console.log(`    Headings mismatch:`);
          console.log(`      Rust: ${JSON.stringify(f.rust_headings)}`);
          console.log(`      JS:   ${JSON.stringify(f.js_headings)}`);
        }
        if (!f.para_count_match) {
          console.log(
            `    Para count: Rust=${f.rust_para_count} JS=${f.js_para_count}`
          );
        }
      }
      if (f.content_score <= 0.9) {
        console.log(`    Content: LOW (score=${f.content_score.toFixed(3)})`);
      }
      console.log();
    }
  }

  // Exit code: 0 only when ALL fixtures have execution OK AND structure + content pass
  const allPass = results.every((r) => r.rust_ok && r.js_ok && r.structure_distance <= 10 && r.content_score > 0.9);
  process.exit(allPass ? 0 : 1);
}

main().catch((e) => {
  console.error("Fatal:", e);
  process.exit(1);
});
