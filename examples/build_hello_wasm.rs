fn main() {
    let text = std::fs::read_to_string("examples/plugin/hello-wasm/hello.wat")
        .expect("examples/plugin/hello-wasm/hello.wat not found");
    let binary = wat::parse_str(&text).expect("WAT parse failed");
    let out = "examples/plugin/hello-wasm/hello.wasm";
    std::fs::write(out, &binary).unwrap();
    println!("wrote {} bytes → {out}", binary.len());
}
