use std::time::Duration;

use synthizer::sample_sources;
use synthizer::*;

fn main() -> Result<()> {
    env_logger::init();

    let mut synth = Synthesizer::new_default_output()?;

    let args = std::env::args().collect::<Vec<_>>();
    let file_path = args
        .get(1)
        .expect("Specify a file path as the first argument");
    let file = std::fs::File::open(file_path).unwrap();
    let source = sample_sources::create_encoded_source(file)?;

    let mut media;

    let delay_line = DelayLineHandle::<[f64; 2]>::new_defaulting(
        std::num::NonZeroUsize::new(synth.duration_to_samples(Duration::from_secs(5))).unwrap(),
    );

    let delconst = Chain::new(synth.duration_to_samples(Duration::from_secs(2)));

    let _handle = {
        let mut batch = synth.batch();

        media = batch.make_media(source)?;

        let m = media.start_chain::<2>(ChannelFormat::Stereo);
        let writer = delay_line.write(m).boxed();
        let reader = delay_line.read(delconst).boxed();
        let full = writer
            .join(reader)
            .to_audio_device(ChannelFormat::Stereo)
            .discard_and_default();

        batch.mount(full)?
    };

    std::thread::sleep(Duration::from_secs(30));
    Ok(())
}
