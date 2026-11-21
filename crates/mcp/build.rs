// Embeds the program's version info on Windows (tools/build/version_info.rs).
fn main() {
    version_info("Mister Download Manager MCP server", None);
}

include!("../../tools/build/version_info.rs");
