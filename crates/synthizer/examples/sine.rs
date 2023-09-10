use std::thread::sleep;
use std::time::Duration;

use synthizer as syz;

fn main() -> syz::Result<()> {
    env_logger::init();

    let server = syz::ServerHandle::new_default_device()?;
    let audio_output = syz::nodes::AudioOutputNodeHandle::new(&server, syz::ChannelFormat::Stereo)?;

    for freq in [300.0f64, 400.0, 500.0] {
        let sin = syz::nodes::TrigWaveformNodeHandle::new_sin(&server, freq)?;
        server.connect(&sin, 0, &audio_output, 0)?;
        sleep(Duration::from_secs(3));
        std::mem::drop(sin);
        sleep(Duration::from_secs(1));
    }

    Ok(())
}
