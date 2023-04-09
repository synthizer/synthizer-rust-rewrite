fn main() {
    cc::Build::new()
        .file("src/c/miniaudio_wrapper.c")
        // This is useless under GCC, which freaks out about unused functions with our static miniaudio.
        .warnings(false)
        .compile("syz_miniaudio_0_1_0");
    println!("cargo:rerun-if-changed=src/c");
}
