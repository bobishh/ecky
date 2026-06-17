use std::fs;
use std::path::PathBuf;

use ecky_cad_lib::mcp::server::export_mcp_tool_catalog;
use ecky_cad_lib::mcp::skill::render_tools_markdown;

fn main() {
    let catalog = export_mcp_tool_catalog();
    let markdown = render_tools_markdown(&catalog);

    let output_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../skills/ecky-mcp/reference/tools.md");
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).expect("Failed to create skill reference directory");
    }
    fs::write(&output_path, markdown).expect("Failed to write generated tools.md");

    eprintln!(
        "Wrote {} tools to {}",
        catalog.len(),
        output_path.display()
    );
}
