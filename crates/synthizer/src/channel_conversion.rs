use arrayvec::ArrayVec;

use crate::config::{BLOCK_SIZE, MAX_CHANNELS};

use crate::data_structures::block_allocator::{AllocatedBlock, BlockAllocator};
use crate::ChannelFormat;

struct ChannelMixingMatrix {
    input_format: ChannelFormat,
    output_format: ChannelFormat,

    /// The outer slice is the input channels; the inner slice the weights to the output channels.
    values: &'static [&'static [f64]],
}

const MIXING_MATRICES: &[ChannelMixingMatrix] = &[
    ChannelMixingMatrix {
        input_format: ChannelFormat::Mono,
        output_format: ChannelFormat::Stereo,
        values: &[&[1.0, 1.0]],
    },
    ChannelMixingMatrix {
        input_format: ChannelFormat::Stereo,
        output_format: ChannelFormat::Mono,
        values: &[&[0.5], &[0.5]],
    },
];

impl ChannelMixingMatrix {
    fn apply(&self, input_buffers: &[AllocatedBlock], output_buffers: &mut [AllocatedBlock]) {
        let ichans = self.input_format.get_channel_count().get();
        let ochans = self.output_format.get_channel_count().get();

        assert_eq!(input_buffers.len(), ichans);
        assert_eq!(output_buffers.len(), ochans);

        for (i, ibuf) in input_buffers.iter().enumerate() {
            for (o, obuf) in output_buffers.iter_mut().enumerate() {
                let weight = self.values[i][o];

                for b in 0..BLOCK_SIZE {
                    obuf[b] += ibuf[b] * weight;
                }
            }
        }
    }
}

fn truncate_or_expand_fallback(
    input_buffers: &[AllocatedBlock],
    output_buffers: &mut [AllocatedBlock],
) {
    for (i_dest, o_dest) in input_buffers.iter().zip(output_buffers.iter_mut()) {
        for (i_ref, o_ref) in i_dest.iter().zip(o_dest.iter_mut()) {
            *o_ref += *i_ref;
        }
    }
}

/// Mix one channel format into another, using allocated block-sized buffers.
///
/// The algorithm is as follows:
///
/// - [ChannelFormat::Mono] to or from [ChannelFormat::Stereo] will merge or split the audio as appropriate.
/// - Anything else either truncates or extends with zeros.
///
/// This function zeros the outputs. If add_to_outputs is true, it instead adds: we use this for, e.g., mixing multiple things into one set of destination buffers.
pub(crate) fn convert_channels(
    input_format: &ChannelFormat,
    output_format: &ChannelFormat,
    input_buffers: &[AllocatedBlock],
    output_buffers: &mut [AllocatedBlock],
    add_to_outputs: bool,
) {
    if !add_to_outputs {
        for o in output_buffers.iter_mut() {
            o.fill(0.0);
        }
    }

    for i in MIXING_MATRICES.iter() {
        if &i.input_format == input_format && &i.output_format == output_format {
            i.apply(input_buffers, output_buffers);
            return;
        }
    }

    truncate_or_expand_fallback(input_buffers, output_buffers);
}

/// Interleave the given blocks to the given output slice, converting from f64 to f32 in the process.
///
/// This is used at the edge of Synthizer, so writes directly instead of adding.
pub fn interleave_blocks(blocks: &mut [AllocatedBlock], destination: &mut [f32]) {
    assert!(!destination.is_empty());
    assert_eq!(destination.len() % blocks.len(), 0);

    let stride = blocks.len();
    blocks.iter_mut().enumerate().for_each(|(channel, block)| {
        for (i, x) in block.iter().copied().enumerate() {
            let offset = i * stride + channel;
            destination[offset] = x as f32;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mono_stereo() {
        let allocator = BlockAllocator::new(10);
        let mut ibuf = allocator.allocate_block();
        let left = allocator.allocate_block();
        let right = allocator.allocate_block();

        ibuf.fill(1.0);

        let mut outputs = [left, right];

        convert_channels(
            &ChannelFormat::Mono,
            &ChannelFormat::Stereo,
            &[ibuf],
            &mut outputs,
            false,
        );

        assert_eq!(&*outputs[0], &[1.0f64; BLOCK_SIZE]);
        assert_eq!(&*outputs[1], &[1.0f64; BLOCK_SIZE]);
    }

    #[test]
    fn stereo_to_mono() {
        let allocator = BlockAllocator::new(5);
        let mut left = allocator.allocate_block();
        let mut right = allocator.allocate_block();
        let out = allocator.allocate_block();

        // They add to 1, so we expect 0.5 out.  We pick these values to be perfect with the mixing matrix: it isn't
        // necessary to use approximate asserts.
        left.fill(0.25);
        right.fill(0.75);

        let mut outputs = [out];

        convert_channels(
            &ChannelFormat::Stereo,
            &ChannelFormat::Mono,
            &[left, right],
            &mut outputs,
            false,
        );

        assert_eq!(&*outputs[0], &[0.5f64; BLOCK_SIZE]);
    }

    #[test]
    fn test_truncating() {
        let allocator = BlockAllocator::new(5);
        let mut i1 = allocator.allocate_block();
        let mut i2 = allocator.allocate_block();
        let mut i3 = allocator.allocate_block();

        i1.fill(1.0);
        i2.fill(2.0);
        i3.fill(3.0);

        let o1 = allocator.allocate_block();
        let o2 = allocator.allocate_block();

        let inputs = [i1, i2, i3];
        let mut outputs = [o1, o2];

        // the test here is just the control path: we don't have any untouched buffers to check.
        convert_channels(
            &ChannelFormat::new_raw(3),
            &ChannelFormat::new_raw(2),
            &inputs,
            &mut outputs,
            false,
        );

        assert_eq!(&*inputs[0], &*outputs[0]);
        assert_eq!(&*inputs[1], &*outputs[1]);
    }

    #[test]
    fn test_expanding() {
        let allocator = BlockAllocator::new(5);
        let mut i1 = allocator.allocate_block();
        let mut i2 = allocator.allocate_block();

        i1.fill(1.0);
        i2.fill(2.0);

        let mut o1 = allocator.allocate_block();
        let mut o2 = allocator.allocate_block();
        let mut o3 = allocator.allocate_block();

        o1.fill(0.0);
        o2.fill(0.0);

        // This channel should not be touched, so give it a sentinel value.
        o3.fill(100.0);

        let inputs = [i1, i2];
        let mut outputs = [o1, o2, o3];

        convert_channels(
            &ChannelFormat::new_raw(2),
            &ChannelFormat::new_raw(3),
            &inputs,
            &mut outputs,
            true,
        );

        assert_eq!(&*outputs[0], &*inputs[0]);
        assert_eq!(&*outputs[1], &*inputs[1]);
        assert_eq!(&*outputs[2], &[100.0f64; BLOCK_SIZE]);
    }

    #[test]
    fn test_interleaving() {
        let allocator = BlockAllocator::new(2);

        let mut left = allocator.allocate_block();
        let mut right = allocator.allocate_block();

        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            *l = (2 * i) as f64;
            *r = (2 * i + 1) as f64;
        }

        let mut got = vec![0.0f32; BLOCK_SIZE * 2];
        interleave_blocks(&mut [left, right], &mut got[..]);

        let expected = (0..BLOCK_SIZE * 2).map(|i| i as f32).collect::<Vec<_>>();
        assert_eq!(got, expected);
    }
}
