use clap::Parser;
use filegoblin_core::cli::Cli;
use filegoblin_core::parsers::gobble::Gobble;
use filegoblin_core::parsers::pdf::PdfGobbler;
use std::path::Path;

#[test]
fn test_benchmark_slices_extract_successfully() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest_dir).join("../../test_assets/benchmarks");

    let slices = [
        "slice_a_twocolumn.pdf",
        "slice_b_tables_charts.pdf",
        "slice_c_callouts_headers.pdf",
    ];

    let gobbler = PdfGobbler;
    let cli = Cli::parse_from(["filegoblin", "dummy"]);
    let opts = (&cli).into();

    for slice in slices {
        let pdf_path = root.join(slice);
        assert!(
            pdf_path.exists(),
            "Benchmark slice must exist: {:?}",
            pdf_path
        );

        let result = gobbler.gobble(&pdf_path, &opts);
        assert!(
            result.is_ok(),
            "Failed to gobble {:?}: {:?}",
            slice,
            result.err()
        );

        let text = result.unwrap();
        assert!(
            !text.trim().is_empty(),
            "Extracted text was empty for {:?}",
            slice
        );
    }
}
