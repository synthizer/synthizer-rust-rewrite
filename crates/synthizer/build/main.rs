mod gen_hrtf;
mod gen_primes;

fn main() {
    println!("cargo:rerun-if-changed=src/datasets/bin_protos");
    gen_primes::gen_primes();
    gen_hrtf::gen_hrtf();
}
