#!/usr/bin/env python3
"""
Diff-fuzz runner comparing delulu-fetch (Rust) output against trafilatura (Python).

Reads explicit .html.zst fixtures from fixtures-webfetch/, runs both
delulu-fetch (via subprocess) and trafilatura.extract (direct Python import),
then computes word-level content similarity and outputs JSON lines to stdout.

Usage:
  cd tests/diff_fuzz
  cargo build --release -p delulu-webfetch
  pip install trafilatura zstd
  python3 run_vs_trafilatura.py
"""

import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

import trafilatura
import zstd

HERE = Path(__file__).resolve().parent
FIXTURE_DIR = HERE.parent / "fixtures-webfetch"
CLI_BINARY = HERE.parent.parent.parent.parent / "target" / "release" / "delulu-fetch"

# Explicit fixture list (no globbing — fixtures were cleaned up in Phase 1)
FIXTURES = [
    "dankrad-pcs-multiproofs.html.zst",
    "ethresear-reed-solomon.html.zst",
]


def decompress_zst(path: Path) -> str:
    """Decompress a .zst file and return the content as a string."""
    with open(path, "rb") as f:
        compressed = f.read()
    decompressed = zstd.decompress(compressed)
    return decompressed.decode("utf-8")


def run_delulu_fetch(html: str) -> str:
    """Run delulu-fetch CLI with the given HTML via stdin, return stdout.

    Raises:
        FileNotFoundError: If the CLI binary does not exist.
        subprocess.TimeoutExpired: If the subprocess exceeds the timeout.
        RuntimeError: If the subprocess exits with a non-zero return code.
    """
    # Timeout is a heuristic; may need adjustment for large inputs.
    result = subprocess.run(
        [str(CLI_BINARY), "-i", "-", "--output-format", "html"],
        input=html,
        capture_output=True,
        text=True,
        timeout=30,
    )
    # Check returncode: a crashed binary with partial stdout produces misleading results.
    if result.returncode != 0:
        raise RuntimeError(
            f"delulu-fetch exited with code {result.returncode}: "
            f"stderr={result.stderr[:500]!r}"
        )
    return result.stdout


def run_trafilatura_extract(html: str) -> str:
    """Run trafilatura.extract directly (Python import, not subprocess)."""
    result = trafilatura.extract(
        html,
        output_format="txt",
        include_tables=False,
        include_images=False,
    )
    return result or ""


def strip_all_html(text: str) -> str:
    """Strip ALL HTML tags from the input, returning raw text content.

    Uses simple regex tag removal (not a full HTML parser).

    Known limitations:
    - Malformed nested angle brackets may cause performance degradation
    - Script/style/SVG content is preserved as text (only tag delimiters <> are removed)
    - Does NOT decode HTML entities

    Args:
        text: HTML string to strip. Must be a string (throws TypeError otherwise).

    Returns:
        Raw text with all tags removed, whitespace normalized, trimmed.
    """
    return re.sub(r"\s+", " ", re.sub(r"<[^>]+>", " ", text)).strip()


def content_similarity(rust_html: str, reference_input: str) -> float:
    """
    Compute word-level multiset edit distance between two HTML/text documents.

    Strips ALL HTML tags from both inputs, tokenizes into lowercase words,
    and computes: 1 - (missingRefWords + extraOutputWords) / refWordCount
    using word-frequency maps (multiset), NOT Set-based dedup.

    Formula asymmetry (known trade-off): Extra output words are penalized as
    harshly as missing reference words. A verbose output containing all reference
    words plus extras may score 0.0.

    Pre-condition: Both inputs are HTML strings (stripAllHtml is called internally).
    Post-condition: Return value in [0.0, 1.0], clamped.
    Throws: TypeError if either argument is not a string.
    Known limitation: /\\W+/ tokenization is unsuitable for CJK/Unicode text.

    Args:
        rust_html: HTML output from Rust pipeline
        reference_input: Reference HTML or plain text

    Returns:
        Content similarity score in [0.0, 1.0]
    """
    rust_text = strip_all_html(rust_html)
    ref_text = strip_all_html(reference_input)

    ref_words = [w for w in re.split(r"\W+", ref_text.lower()) if w]
    out_words = [w for w in re.split(r"\W+", rust_text.lower()) if w]

    if len(ref_words) == 0:
        return 1.0 if len(out_words) == 0 else 0.0

    ref_freq = Counter(ref_words)
    out_freq = Counter(out_words)

    missing_ref_words = 0
    for word, ref_count in ref_freq.items():
        out_count = out_freq.get(word, 0)
        if ref_count > out_count:
            missing_ref_words += ref_count - out_count

    extra_output_words = 0
    for word, out_count in out_freq.items():
        ref_count = ref_freq.get(word, 0)
        if out_count > ref_count:
            extra_output_words += out_count - ref_count

    ref_word_count = len(ref_words)
    score = 1.0 - (missing_ref_words + extra_output_words) / ref_word_count
    return max(0.0, min(1.0, score))


def main():
    # Precondition checks: crash with clear diagnostics for missing dependencies
    if not CLI_BINARY.exists():
        print(f"FATAL: CLI binary not found at {CLI_BINARY}", file=sys.stderr)
        print("Build the project first with: cargo build --release", file=sys.stderr)
        sys.exit(1)

    results = []

    for fixture in FIXTURES:
        fixture_path = FIXTURE_DIR / fixture

        if not fixture_path.exists():
            result = {
                "fixture": fixture,
                "rust_ok": False,
                "tf_ok": False,
                "structure_distance": 0,
                "content_score": 0.0,
                "error": f"fixture not found: {fixture_path}",
            }
            results.append(result)
            print(json.dumps(result))
            continue

        # Decompress
        try:
            html = decompress_zst(fixture_path)
        except (zstd.Error, OSError, UnicodeDecodeError) as e:
            result = {
                "fixture": fixture,
                "rust_ok": False,
                "tf_ok": False,
                "structure_distance": 0,
                "content_score": 0.0,
                "error": f"decompress failed: {e}",
            }
            results.append(result)
            print(json.dumps(result))
            continue

        # Run delulu-fetch
        rust_output = ""
        rust_ok = False
        rust_output_error = ""
        try:
            rust_output = run_delulu_fetch(html)
            rust_ok = True
        except subprocess.TimeoutExpired as e:
            rust_output_error = f"Rust pipeline timed out after {e.timeout}s"
            print(f"  {fixture}: {rust_output_error}", file=sys.stderr)
        except (FileNotFoundError, RuntimeError) as e:
            rust_output_error = f"Rust pipeline failed: {e}"
            print(f"  {fixture}: {rust_output_error}", file=sys.stderr)

        # Run trafilatura
        tf_output = ""
        tf_ok = False
        tf_output_error = ""
        try:
            tf_output = run_trafilatura_extract(html)
            tf_ok = True
        except Exception as e:
            tf_output_error = f"Trafilatura extraction failed: {e}"
            print(f"  {fixture}: {tf_output_error}", file=sys.stderr)

        content_score = content_similarity(rust_output, tf_output)
        rust_blocks = len([b for b in rust_output.split("\n\n") if b.strip()])
        tf_blocks = len([b for b in tf_output.split("\n\n") if b.strip()])
        struct_dist = abs(rust_blocks - tf_blocks)

        result = {
            "fixture": fixture,
            "rust_ok": rust_ok,
            "tf_ok": tf_ok,
            "structure_distance": struct_dist,
            "content_score": content_score,
        }
        if not rust_ok:
            result["error"] = rust_output_error
        elif not tf_ok:
            result["error"] = tf_output_error

        results.append(result)
        print(json.dumps(result))

    # Summary to stderr so stdout remains clean JSONL
    passed = 0
    threshold = 0.9
    for r in results:
        ok = r["rust_ok"] and r["tf_ok"] and r["content_score"] > threshold
        if ok:
            passed += 1
        icon = "\u2713" if ok else "\u2717"
        error_info = f" error={r.get('error', '')}" if r.get("error") else ""
        print(
            f"  {icon} {r['fixture']}: "
            f"struct={r['structure_distance']} "
            f"cont={r['content_score']:.3f}"
            f"{error_info}",
            file=sys.stderr,
        )

    print(f"\n  {passed}/{len(results)} passed (threshold: {threshold})", file=sys.stderr)

    all_pass = all(
        r["rust_ok"] and r["tf_ok"] and r["content_score"] > threshold
        for r in results
    )
    sys.exit(0 if all_pass else 1)


if __name__ == "__main__":
    main()
