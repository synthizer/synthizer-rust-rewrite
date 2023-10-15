use std::thread::sleep;
use std::time::Duration;

use synthizer as syz;

fn main() -> syz::Result<()> {
    env_logger::init();

    let server = syz::Server::new_default_device()?;
    let audio_output = syz::nodes::AudioOutputNodeHandle::new(&server, syz::ChannelFormat::Stereo)?;

    let sin = syz::nodes::TrigWaveformNodeHandle::new_sin(&server, 300.0)?;
    server.connect(&sin, 0, &audio_output, 0)?;
    sleep(Duration::from_millis(500));

    for freq in [400.0, 500.0] {
        sin.props().frequency().set_value(freq)?;
        sleep(Duration::from_millis(500));
    }

    Ok(())
}
