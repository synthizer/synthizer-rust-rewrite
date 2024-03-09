use anyhow::Result;
use std::time::Duration;

use synthizer as syz;

use crate::test_config::TestConfig;
use crate::validators::Validator;

pub struct TestContext {
    pub test_name: String,
    pub channel_format: syz::ChannelFormat,
    pub server: syz::Server,
    pub validators: Vec<Box<dyn Validator>>,

    /// Last chunk of synthesized audio, if any.
    ///
    /// On failure to advance, set to the empty vec.
    pub synthesized_audio: Vec<f32>,
}

/// Trait representing various units of time.
pub trait Advanceable {
    fn get_frame_count(&self) -> usize;
}

/// Advance by some number of frames.
pub struct Frames(pub usize);

/// Advance by some number of blocks.
pub struct Blocks(pub usize);

impl Advanceable for Frames {
    fn get_frame_count(&self) -> usize {
        self.0
    }
}

impl Advanceable for Blocks {
    fn get_frame_count(&self) -> usize {
        self.0.checked_mul(syz::BLOCK_SIZE).expect(
            "Attempt to advance by more than the number of frames that could ever fit into memory",
        )
    }
}

impl Advanceable for Duration {
    fn get_frame_count(&self) -> usize {
        let secs = self.as_secs_f64();
        let frames = secs * (syz::SR as f64);
        (frames.ceil() as u64).try_into().expect("Unable to advance by this many seconds because that would require more frames than can fit into the memory of this machine")
    }
}
impl TestContext {
    pub fn from_config(test_name: &str, config: TestConfig) -> Result<TestContext> {
        let mut ret = Self {
            channel_format: syz::ChannelFormat::Stereo,
            server: syz::Server::new_inline()
                .expect("Must be able to create a Synthizer server for test startup"),
            test_name: test_name.to_string(),
            validators: vec![],
            synthesized_audio: vec![0.0; syz::BLOCK_SIZE * 2],
        };

        let validators = config
            .validators
            .into_iter()
            .map(|x| x.build_validator(&ret))
            .collect::<Vec<_>>();
        ret.validators = validators;

        Ok(ret)
    }

    /// Run the specified closure over all validators in such a way as to allow it to itself get a context.
    ///
    /// Deals with the limitation of Rust that we cannot do field splitrting over the whole call grapha by taking the
    /// validators out of the context, then putting them back.
    fn validators_foreach(&mut self, mut callback: impl FnMut(&TestContext, &mut dyn Validator)) {
        let mut vals = std::mem::take(&mut self.validators);
        for v in vals.iter_mut() {
            callback(self, &mut **v);
        }
        self.validators = vals;
    }

    /// Get the outcomes of all validators.
    pub fn finalize_validators(&mut self) -> Vec<Result<(), crate::validators::ValidatorFailure>> {
        let mut ret = vec![];
        self.validators_foreach(|ctx, v| {
            ret.push(v.finalize(ctx));
        });
        ret
    }

    #[track_caller]
    fn advance_by_frames(&mut self, frame_count: usize) -> Result<()> {
        let mut synthesized_audio = std::mem::take(&mut self.synthesized_audio);
        synthesized_audio.resize(
            frame_count * self.channel_format.get_channel_count().get(),
            0.0,
        );
        // We want to test that Synthizer zeros this buffer, so we can fill it with NaN and then tests freak out if this
        // is not the case.
        synthesized_audio.fill(f32::NAN);

        self.server.synthesize_stereo(&mut synthesized_audio[..])?;

        // Must be here, otherwise track_caller doesn't work through the closure.
        let loc = std::panic::Location::caller();
        self.validators_foreach(|ctx, v| {
            v.validate_batched(ctx, loc, &synthesized_audio[..]);
        });
        Ok(())
    }

    /// Advance this simulation.  Time may be:
    ///
    /// - A [Duration]. This is converted to samples and rounded up to the next sample.
    /// - [Frames]: E.g. `Frames(2)`.  Advance by the specified number of frames.
    /// - [Blocks]: e.g. `Blocks(5)`.  Advance by the specified number of blocks.
    #[track_caller]
    pub fn advance<T: Advanceable>(&mut self, time: T) -> Result<()> {
        self.advance_by_frames(time.get_frame_count())
    }
}
