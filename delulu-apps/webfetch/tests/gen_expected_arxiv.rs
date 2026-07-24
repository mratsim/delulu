//! Helper binary: run with `cargo test -p delulu-webfetch --test gen_expected_arxiv -- --nocapture`
//! to regenerate expected.md.zst for all arXiv pipeline test fixtures.
#[test]
#[ignore = "manual: regenerates golden fixtures; run explicitly"]
fn generate_expected_arxiv_output() {
    generate_for("attention-is-all-you-need");
    generate_for("valida-isa");
}

fn generate_for(name: &str) {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest.join(format!("tests/fixtures-arxiv/{name}/source.html.zst"));

    let compressed = std::fs::read(&fixture_path).unwrap();
    let decompressed = zstd::decode_all(compressed.as_slice()).unwrap();
    let html = String::from_utf8(decompressed).unwrap();

    let mut dom = delulu_webfetch::pipelines::parse_html(&html).unwrap();
    delulu_webfetch::pipelines::dl_arxiv::filter_arxiv(&mut dom);
    let md = delulu_webfetch::generators::gen_md::MarkdownLowerer::lower(&dom, None);

    let out_path = manifest.join(format!("tests/fixtures-arxiv/{name}/expected.md.zst"));
    let compressed_out = zstd::encode_all(md.as_bytes(), 3).unwrap();
    std::fs::write(&out_path, &compressed_out).unwrap();
    eprintln!(
        "Written {} bytes -> {}",
        compressed_out.len(),
        out_path.display()
    );
    eprintln!("Markdown: {} chars", md.len());
}
