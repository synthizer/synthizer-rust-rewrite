mod gen_hrtf;
mod gen_primes;

fn main() {
    gen_primes::gen_primes();
    gen_hrtf::gen_hrtf();
}
