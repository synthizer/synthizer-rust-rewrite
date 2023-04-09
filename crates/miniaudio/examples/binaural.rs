use std::io::Write;
use std::num::NonZeroU32;

use syz_miniaudio::*;

const SR: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(44100) };
const FREQ_L: f64 = 300.0;
const FREQ_R: f64 = 305.0;

struct SinState {
    phase: f64,
}

impl SinState {
    fn advance(&mut self) -> (f32, f32) {
        use std::f64::consts::PI;
        let left = (2.0 * PI * FREQ_L * self.phase).sin();
        let right = (2.0 * PI * FREQ_R * self.phase).sin();
        self.phase += 1.0 / (SR.get() as f64);
        self.phase = self.phase - self.phase.floor();
        (left as f32, right as f32)
    }
}

fn main() -> Result<()> {
    env_logger::init();

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    let device_opts = DeviceOptions {
        sample_rate: Some(SR),
        channel_format: Some(DeviceChannelFormat::Stereo),
    };

    let devices = enumerate_output_devices()?.collect::<Vec<_>>();

    println!("0: Use system default");
    for (i, d) in devices.iter().enumerate() {
        println!("{}: {}", i + 1, d.name());
    }

    print!("Select device... ");
    stdout.flush().unwrap();

    let mut line = String::new();

    stdin.read_line(&mut line).unwrap();

    let ind: usize = line.trim().parse().unwrap();

    if ind > devices.len() {
        panic!("Device index out of range");
    }

    let mut cb_state = SinState { phase: 0.0 };
    let cb = move |cfg: &DeviceConfig, dest: &mut [f32]| {
        assert!(cfg.channels() == 2);
        let will_do = dest.len() / 2;
        for i in 0..will_do {
            let li = i * 2;
            let ri = li + 1;
            let (left, right) = cb_state.advance();
            dest[li] = left;
            dest[ri] = right;
        }
    };

    let mut device = if ind == 0 {
        open_default_playback_device(&device_opts, cb)?
    } else {
        open_playback_device(&devices[ind - 1], &device_opts, cb)?
    };

    let mut running = true;
    device.start()?;

    println!("Press enter to play and pause...");
    loop {
        stdin.read_line(&mut line).unwrap();
        line.clear();

        if running {
            device.stop()?;
        } else {
            device.start()?;
        }
        running = !running;
    }
}
