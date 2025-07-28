use std::time::Duration;

use synthizer::*;

fn main() -> Result<()> {
    env_logger::init();

    let mut synth = Synthesizer::new_default_output()?;

    let args = std::env::args().collect::<Vec<_>>();
    let file_path = args
        .get(1)
        .expect("Specify a file path as the first argument");
    let file = std::fs::File::open(file_path).unwrap();

    let (controller, mut media);

    let handle = {
        let mut batch = synth.batch();

        (controller, media) = batch.make_media(file)?;

        batch.mount(
            media
                .start_chain::<2>(ChannelFormat::Stereo)
                .to_audio_device(ChannelFormat::Stereo),
        )?
    };

    println!(
        "
Commands:

play
pause
quit
seek <secs>: seek to a position in the file.
loop (off | full | range) [<start> <end>]: configure looping.
"
    );

    controller.play()?;

    loop {
        use std::io::Write;

        print!("Command> ");
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        // trim removes \r\n.
        let mut line = line.trim().split(' ');

        let cmd = line.next();
        let Some(cmd) = cmd else {
            continue;
        };

        match cmd {
            "quit" => break,
            "s" | "seek" => {
                let Some(pos) = line.next() else {
                    println!("Missing parameter position");
                    continue;
                };

                let Ok(pos) = pos.parse::<f64>() else {
                    println!("Position not a number: {pos:?}");
                    continue;
                };

                let batch = synth.batch();
                controller.seek((pos * controller.get_sr() as f64) as u64)?;
            }
            "pause" => {
                controller.pause()?;
            }
            "play" => {
                controller.play()?;
            }
            "loop" => {
                let Some(ltype) = line.next() else {
                    println!("Missing loop subcommand");
                    continue;
                };

                let spec = match ltype {
                    "off" => LoopSpec::none(),
                    "full" => LoopSpec::all(),
                    "range" => {
                        let Some(start) = line.next() else {
                            println!("Missing start");
                            continue;
                        };

                        let Some(end) = line.next() else {
                            println!("Missing end");
                            continue;
                        };

                        let Ok(start) = start.parse::<f64>() else {
                            println!("Start not a number: {start:?}");
                            continue;
                        };

                        let Ok(end) = end.parse::<f64>() else {
                            println!("End not a number: {end:?}");
                            continue;
                        };

                        LoopSpec::timestamps(
                            Duration::from_secs_f64(start),
                            Some(Duration::from_secs_f64(end)),
                        )
                    }
                    _ => {
                        println!("Unrecognized loop type {ltype}");
                        continue;
                    }
                };

                controller.set_looping(spec)?;
            }
            _ => {
                println!("Unrecognized command {cmd}");
                continue;
            }
        }
    }

    Ok(())
}
