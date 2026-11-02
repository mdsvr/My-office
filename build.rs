// The dashboard (web/dist) is embedded into the binary; rebuild when it changes,
// including when Vite renames its hashed asset files.
fn main() {
    println!("cargo:rerun-if-changed=web/dist");
}
