use synthizer::*;

fn main() {
    let pi2 = 2.0f64 * std::f64::consts::PI;
    let chain1 = Chain::new(500f64)
        .divide_by_sr()
        .periodic_sum(pi2, 0.0f64)
        .sin();
    let chain2 = Chain::new(600f64)
        .divide_by_sr()
        .periodic_sum(pi2, 0.0)
        .sin();
    let added = chain1 + chain2;
    let ready = added * Chain::new(0.5f64);
    let to_dev = ready.to_audio_device();

    let mut synth = Synthesizer::new_audio_defaults();
    let _handle = {
        let mut batch = synth.batch();
        batch.mount(to_dev)
    };

    std::thread::sleep(std::time::Duration::from_secs(5));
}
