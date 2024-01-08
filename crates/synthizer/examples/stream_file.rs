use std::time::Duration;

use synthizer as syz;

fn main() -> syz::Result<()> {
    env_logger::init();

    let server = syz::Server::new_default_device()?;
    let audio_output = syz::nodes::AudioOutputNode::new(&server, syz::ChannelFormat::Stereo)?;

    let args = std::env::args().collect::<Vec<_>>();
    let file_path = args
        .get(1)
        .expect("Specify a file path as the first argument");
    let file = std::fs::File::open(file_path).unwrap();
    let source: Box<dyn syz::sample_sources::SampleSource> =
        syz::sample_sources::create_encoded_source(file).unwrap();
    let player = syz::nodes::SampleSourcePlayerNode::new(&server, source)?;

    player.config_looping(syz::LoopSpec::timestamps(
        Duration::from_secs(1),
        Some(Duration::from_secs(3)),
    ))?;

    server.connect(&player, 0, &audio_output, 0)?;

    println!("Ctrl+c or enter to exit");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    Ok(())
}
