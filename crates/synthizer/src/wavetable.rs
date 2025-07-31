use std::io::{Read, Seek};
use std::sync::Arc;

use crate::channel_format::ChannelFormat;
use crate::core_traits::AudioFrame;
use crate::error::Result;
use crate::mark_dropped::MarkDropped;
use crate::sample_sources::UnifiedMediaSource;
use crate::unique_id::UniqueId;

pub struct WaveTable {
    data: Vec<f32>,
    channel_format: ChannelFormat,
    sample_rate: u32,
    frame_count: usize,
}

pub struct WaveTableBuilder<S> {
    source: Option<S>,
    channel_format: Option<ChannelFormat>,
    target_sample_rate: u32,
}

impl<S> WaveTableBuilder<S>
where
    S: Read + Seek + Send + Sync + 'static,
{
    pub fn new(source: S) -> Self {
        Self {
            source: Some(source),
            channel_format: None,
            target_sample_rate: crate::config::SR as u32,
        }
    }

    pub fn channel_format(mut self, format: ChannelFormat) -> Self {
        self.channel_format = Some(format);
        self
    }

    pub fn target_sample_rate(mut self, rate: u32) -> Self {
        self.target_sample_rate = rate;
        self
    }

    pub fn build(mut self) -> Result<WaveTable> {
        let source = self.source.take().expect("Builder can only be used once");

        let mut media = UnifiedMediaSource::new(source, self.target_sample_rate)?;

        let descriptor = media.get_descriptor().clone();
        let channel_format = self.channel_format.unwrap_or(descriptor.channel_format);
        let channels = channel_format.get_channel_count().get();

        let initial_capacity = 44100 * channels;
        let mut data = Vec::with_capacity(initial_capacity);

        loop {
            let start_len = data.len();
            data.resize(start_len + (crate::config::BLOCK_SIZE * channels), 0.0);

            let frames_read = media.read_samples(&mut data[start_len..])? as usize;

            if frames_read == 0 {
                data.truncate(start_len);
                break;
            }

            let samples_read = frames_read * channels;
            data.truncate(start_len + samples_read);
        }

        data.shrink_to_fit();

        let frame_count = data.len() / channels;

        Ok(WaveTable {
            data,
            channel_format,
            sample_rate: self.target_sample_rate,
            frame_count,
        })
    }

    pub fn build_with_batch(
        self,
        batch: &mut crate::synthesizer::Batch,
    ) -> Result<WaveTableHandle> {
        let wavetable = self.build()?;
        let wavetable_id = UniqueId::new();
        let mark_drop = MarkDropped::new();

        let wavetable_arc = Arc::new(wavetable);
        let mut wavetable_opt = Some((wavetable_arc.clone(), mark_drop.0.clone()));

        batch.push_command(move |state: &mut crate::synthesizer::AudioThreadState| {
            if let Some((wavetable, pending_drop)) = wavetable_opt.take() {
                state.wavetables.insert(wavetable_id, (wavetable, pending_drop));
            }
        });

        Ok(WaveTableHandle {
            wavetable_id,
            wavetable: wavetable_arc,
            mark_drop: Arc::new(mark_drop),
        })
    }
}

impl WaveTable {
    pub fn builder<S: Read + Seek + Send + Sync + 'static>(source: S) -> WaveTableBuilder<S> {
        WaveTableBuilder::new(source)
    }

    pub fn channel_count(&self) -> usize {
        self.channel_format.get_channel_count().get()
    }

    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Wraps or clamps an index based on the LOOPING parameter
    #[inline(always)]
    fn constrain_index<const LOOPING: bool>(&self, index: isize) -> usize {
        if LOOPING {
            index.rem_euclid(self.frame_count as isize) as usize
        } else {
            index.clamp(0, self.frame_count.saturating_sub(1) as isize) as usize
        }
    }

    /// Get a sample value at the given frame and channel indices
    #[inline(always)]
    fn get_sample(&self, frame_index: usize, channel: usize) -> f64 {
        self.data[frame_index * self.channel_count() + channel] as f64
    }

    /// Fill a frame with samples from the given index
    #[inline(always)]
    fn fill_frame<F: AudioFrame<f64>>(&self, frame_index: usize) -> F {
        let mut frame = F::default_frame();
        let channels = self.channel_count();

        for ch in 0..channels {
            frame.set_or_ignore(ch, self.get_sample(frame_index, ch));
        }

        frame
    }

    #[inline(always)]
    pub fn read_truncated<F: AudioFrame<f64>, const LOOPING: bool>(&self, position: f64) -> F {
        if self.frame_count == 0 {
            return F::default_frame();
        }

        let index = self.constrain_index::<LOOPING>(position.floor() as isize);
        self.fill_frame(index)
    }

    #[inline(always)]
    pub fn read_linear<F: AudioFrame<f64>, const LOOPING: bool>(&self, position: f64) -> F {
        if self.frame_count == 0 {
            return F::default_frame();
        }

        // For non-looping mode, return default frame if out of bounds
        if !LOOPING && (position < 0.0 || position >= self.frame_count as f64) {
            return F::default_frame();
        }

        let base_position = if LOOPING {
            position.rem_euclid(self.frame_count as f64)
        } else {
            position
        };

        let index = base_position.floor() as isize;
        let fraction = base_position - index as f64;

        let idx0 = self.constrain_index::<LOOPING>(index);
        let idx1 = self.constrain_index::<LOOPING>(index + 1);

        let mut frame = F::default_frame();
        let channels = self.channel_count();

        for ch in 0..channels {
            let s0 = self.get_sample(idx0, ch);
            let s1 = self.get_sample(idx1, ch);
            frame.set_or_ignore(ch, s0 + (s1 - s0) * fraction);
        }

        frame
    }

    #[inline(always)]
    pub fn read_cubic<F: AudioFrame<f64>, const LOOPING: bool>(&self, position: f64) -> F {
        if self.frame_count == 0 {
            return F::default_frame();
        }

        // For non-looping mode, return default frame if out of bounds
        if !LOOPING && (position < 0.0 || position >= self.frame_count as f64) {
            return F::default_frame();
        }

        let base_position = if LOOPING {
            position.rem_euclid(self.frame_count as f64)
        } else {
            position
        };

        let index = base_position.floor() as isize;
        let t = base_position - index as f64;

        // Get four sample points for cubic interpolation
        let idx0 = self.constrain_index::<LOOPING>(index - 1);
        let idx1 = self.constrain_index::<LOOPING>(index);
        let idx2 = self.constrain_index::<LOOPING>(index + 1);
        let idx3 = self.constrain_index::<LOOPING>(index + 2);

        let mut frame = F::default_frame();
        let channels = self.channel_count();

        // Pre-compute powers of t outside the channel loop
        // This optimization is important for multi-channel audio
        let t2 = t * t;
        let t3 = t2 * t;

        for ch in 0..channels {
            let s0 = self.get_sample(idx0, ch);
            let s1 = self.get_sample(idx1, ch);
            let s2 = self.get_sample(idx2, ch);
            let s3 = self.get_sample(idx3, ch);

            // Inline cubic interpolation to avoid function call overhead
            let a0 = s3 - s2 - s0 + s1;
            let a1 = s0 - s1 - a0;
            let a2 = s2 - s0;
            let a3 = s1;

            frame.set_or_ignore(ch, a0 * t3 + a1 * t2 + a2 * t + a3);
        }

        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_wavetable(data: Vec<f32>, channels: usize) -> WaveTable {
        let channel_format = if channels == 1 {
            ChannelFormat::Mono
        } else if channels == 2 {
            ChannelFormat::Stereo
        } else {
            ChannelFormat::new_raw(channels)
        };

        let frame_count = data.len() / channels;

        WaveTable {
            data,
            channel_format,
            sample_rate: 44100,
            frame_count,
        }
    }

    #[test]
    fn test_read_truncated_mono() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let wt = create_test_wavetable(data, 1);

        let frame: f64 = wt.read_truncated::<_, false>(0.4);
        assert_eq!(frame, 1.0); // floor(0.4) = 0, data[0] = 1.0

        let frame: f64 = wt.read_truncated::<_, false>(0.5);
        assert_eq!(frame, 1.0); // floor(0.5) = 0, data[0] = 1.0

        let frame: f64 = wt.read_truncated::<_, false>(1.6);
        assert_eq!(frame, 2.0); // floor(1.6) = 1, data[1] = 2.0

        let frame: f64 = wt.read_truncated::<_, false>(4.9);
        assert_eq!(frame, 5.0); // floor(4.9) = 4, data[4] = 5.0

        let frame: f64 = wt.read_truncated::<_, false>(100.0);
        assert_eq!(frame, 5.0);
    }

    #[test]
    fn test_read_truncated_stereo() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let wt = create_test_wavetable(data, 2);

        let frame: [f64; 2] = wt.read_truncated::<_, false>(0.4);
        assert_eq!(frame, [1.0, 2.0]); // floor(0.4) = 0

        let frame: [f64; 2] = wt.read_truncated::<_, false>(0.5);
        assert_eq!(frame, [1.0, 2.0]); // floor(0.5) = 0

        let frame: [f64; 2] = wt.read_truncated::<_, false>(1.6);
        assert_eq!(frame, [3.0, 4.0]); // floor(1.6) = 1

        let frame: [f64; 2] = wt.read_truncated::<_, false>(100.0);
        assert_eq!(frame, [5.0, 6.0]);
    }

    #[test]
    fn test_read_linear_mono() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let wt = create_test_wavetable(data, 1);

        let frame: f64 = wt.read_linear::<_, false>(0.0);
        assert_eq!(frame, 1.0);

        let frame: f64 = wt.read_linear::<_, false>(0.5);
        assert_eq!(frame, 1.5);

        let frame: f64 = wt.read_linear::<_, false>(1.0);
        assert_eq!(frame, 2.0);

        let frame: f64 = wt.read_linear::<_, false>(1.25);
        assert_eq!(frame, 2.25);

        let frame: f64 = wt.read_linear::<_, false>(4.0);
        assert_eq!(frame, 5.0);

        let frame: f64 = wt.read_linear::<_, false>(100.0);
        assert_eq!(frame, 0.0);

        let frame: f64 = wt.read_linear::<_, false>(-1.0);
        assert_eq!(frame, 0.0);
    }

    #[test]
    fn test_read_linear_stereo() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let wt = create_test_wavetable(data, 2);

        let frame: [f64; 2] = wt.read_linear::<_, false>(0.0);
        assert_eq!(frame, [1.0, 2.0]);

        let frame: [f64; 2] = wt.read_linear::<_, false>(0.5);
        assert_eq!(frame, [2.0, 3.0]);

        let frame: [f64; 2] = wt.read_linear::<_, false>(1.0);
        assert_eq!(frame, [3.0, 4.0]);

        let frame: [f64; 2] = wt.read_linear::<_, false>(1.5);
        assert_eq!(frame, [4.0, 5.0]);

        let frame: [f64; 2] = wt.read_linear::<_, false>(2.0);
        assert_eq!(frame, [5.0, 6.0]);
    }

    #[test]
    fn test_read_cubic_mono() {
        let data = vec![0.0, 1.0, 4.0, 9.0, 16.0];
        let wt = create_test_wavetable(data, 1);

        let frame: f64 = wt.read_cubic::<_, false>(0.0);
        assert_eq!(frame, 0.0);

        let frame: f64 = wt.read_cubic::<_, false>(1.0);
        assert_eq!(frame, 1.0);

        let frame: f64 = wt.read_cubic::<_, false>(2.0);
        assert_eq!(frame, 4.0);

        let frame: f64 = wt.read_cubic::<_, false>(1.5);
        assert!(frame > 1.0 && frame < 4.0);
    }

    #[test]
    fn test_read_cubic_edge_cases() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let wt = create_test_wavetable(data, 1);

        let frame: f64 = wt.read_cubic::<_, false>(-1.0);
        assert_eq!(frame, 0.0);

        let frame: f64 = wt.read_cubic::<_, false>(0.0);
        assert_eq!(frame, 1.0);

        let frame: f64 = wt.read_cubic::<_, false>(3.0);
        assert_eq!(frame, 4.0);

        let frame: f64 = wt.read_cubic::<_, false>(3.5);
        let _last_frame: f64 = wt.read_cubic::<_, false>(3.0);
        assert!(frame.is_finite());
    }

    #[test]
    fn test_different_channel_counts() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let wt = create_test_wavetable(data.clone(), 3);

        assert_eq!(wt.channel_count(), 3);
        assert_eq!(wt.frame_count(), 2);

        let frame: [f64; 3] = wt.read_truncated::<_, false>(0.0);
        assert_eq!(frame, [1.0, 2.0, 3.0]);

        let frame: [f64; 3] = wt.read_truncated::<_, false>(1.0);
        assert_eq!(frame, [4.0, 5.0, 6.0]);

        let frame: [f64; 2] = wt.read_truncated::<_, false>(0.0);
        assert_eq!(frame, [1.0, 2.0]);

        let frame: [f64; 4] = wt.read_truncated::<_, false>(0.0);
        assert_eq!(frame, [1.0, 2.0, 3.0, 0.0]);
    }

    #[test]
    fn test_looping_truncated() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let wt = create_test_wavetable(data, 1);

        // Test wraparound
        let frame: f64 = wt.read_truncated::<_, true>(-1.0);
        assert_eq!(frame, 4.0); // floor(-1.0) = -1, wraps to index 3

        let frame: f64 = wt.read_truncated::<_, true>(-0.5);
        assert_eq!(frame, 4.0); // floor(-0.5) = -1, wraps to index 3

        let frame: f64 = wt.read_truncated::<_, true>(4.0);
        assert_eq!(frame, 1.0); // wraps to index 0

        let frame: f64 = wt.read_truncated::<_, true>(5.5);
        assert_eq!(frame, 2.0); // floor(5.5) = 5, wraps to index 1
    }

    #[test]
    fn test_looping_linear() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let wt = create_test_wavetable(data, 1);

        // Test wraparound at boundaries
        let frame: f64 = wt.read_linear::<_, true>(-0.5);
        assert_eq!(frame, 2.5); // -0.5 wraps to 3.5, interpolate between index 3 (value 4) and 0 (value 1)

        let frame: f64 = wt.read_linear::<_, true>(3.5);
        assert_eq!(frame, 2.5); // interpolate between index 3 (value 4) and 0 (value 1)

        let frame: f64 = wt.read_linear::<_, true>(4.0);
        assert_eq!(frame, 1.0); // exactly at wrapped index 0

        let frame: f64 = wt.read_linear::<_, true>(4.5);
        assert_eq!(frame, 1.5); // interpolate between index 0 and 1
    }

    #[test]
    fn test_looping_cubic() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let wt = create_test_wavetable(data, 1);

        // Test that it wraps correctly and produces finite values
        let frame: f64 = wt.read_cubic::<_, true>(-0.5);
        assert!(frame.is_finite());

        let frame: f64 = wt.read_cubic::<_, true>(0.0);
        assert_eq!(frame, 1.0);

        let frame: f64 = wt.read_cubic::<_, true>(4.5);
        assert!(frame.is_finite());

        let frame: f64 = wt.read_cubic::<_, true>(5.0);
        assert_eq!(frame, 1.0); // wraps to beginning
    }
}

/// A handle to a wavetable that can be used to create signals that read from it
pub struct WaveTableHandle {
    pub(crate) wavetable_id: UniqueId,
    pub(crate) wavetable: Arc<WaveTable>,
    pub(crate) mark_drop: Arc<MarkDropped>,
}

impl WaveTableHandle {
    /// Get the ID of this wavetable
    pub(crate) fn id(&self) -> UniqueId {
        self.wavetable_id
    }

    /// Get the number of frames in this wavetable
    pub fn frame_count(&self) -> usize {
        self.wavetable.frame_count()
    }

    /// Get the sample rate of this wavetable
    pub fn sample_rate(&self) -> u32 {
        self.wavetable.sample_rate()
    }

    /// Get the number of channels in this wavetable
    pub fn channel_count(&self) -> usize {
        self.wavetable.channel_count()
    }

    /// Create a signal that reads from this wavetable using truncation (no interpolation)
    pub fn read_truncated<F, const LOOPING: bool>(
        &self,
        increment: f64,
    ) -> impl crate::core_traits::IntoSignal<
        Signal = impl crate::core_traits::Signal<Input = (), Output = F>,
    >
    where
        F: crate::core_traits::AudioFrame<f64> + Send + Sync + 'static,
    {
        crate::signals::WaveTableTruncatedSignalConfig::<F, LOOPING> {
            wavetable: self.wavetable.clone(),
            increment,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Create a signal that reads from this wavetable using linear interpolation
    pub fn read_linear<F, const LOOPING: bool>(
        &self,
        increment: f64,
    ) -> impl crate::core_traits::IntoSignal<
        Signal = impl crate::core_traits::Signal<Input = (), Output = F>,
    >
    where
        F: crate::core_traits::AudioFrame<f64> + Send + Sync + 'static,
    {
        crate::signals::WaveTableLinearSignalConfig::<F, LOOPING> {
            wavetable: self.wavetable.clone(),
            increment,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Create a signal that reads from this wavetable using cubic interpolation
    pub fn read_cubic<F, const LOOPING: bool>(
        &self,
        increment: f64,
    ) -> impl crate::core_traits::IntoSignal<
        Signal = impl crate::core_traits::Signal<Input = (), Output = F>,
    >
    where
        F: crate::core_traits::AudioFrame<f64> + Send + Sync + 'static,
    {
        crate::signals::WaveTableCubicSignalConfig::<F, LOOPING> {
            wavetable: self.wavetable.clone(),
            increment,
            _phantom: std::marker::PhantomData,
        }
    }
}
