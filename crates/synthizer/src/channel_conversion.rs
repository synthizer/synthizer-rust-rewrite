use arrayvec::ArrayVec;

use crate::config::{BLOCK_SIZE, MAX_CHANNELS};

use crate::data_structures::block_allocator::{AllocatedBlock, BlockAllocator};
use crate::ChannelFormat;

struct ChannelMixingMatrix {
    input_format: ChannelFormat,
    output_format: ChannelFormat,

    /// The outer slice is the input channels; the inner slice the weights to the output channels.
    values: &'static [&'static [f32]],
}

const MIXING_MATRICES: &[ChannelMixingMatrix] = &[
    ChannelMixingMatrix {
        input_format: ChannelFormat::Mono,
        output_format: ChannelFormat::Stereo,
        values: &[&[1.0f32, 1.0f32]],
    },
    ChannelMixingMatrix {
        input_format: ChannelFormat::Stereo,
        output_format: ChannelFormat::Mono,
        values: &[&[0.5], &[0.5]],
    },
];

impl ChannelMixingMatrix {
    fn apply(
        &self,
        allocator: &mut BlockAllocator,
        input_buffers: &mut [&mut AllocatedBlock],
        output_buffers: &mut [&mut AllocatedBlock],
    ) {
        let ichans = self.input_format.get_channel_count().get();
        let ochans = self.output_format.get_channel_count().get();

        assert_eq!(input_buffers.len(), ichans);
        assert_eq!(output_buffers.len(), ochans);

        for (i, ibuf) in input_buffers.iter_mut().enumerate() {
            for (o, obuf) in output_buffers.iter_mut().enumerate() {
                let i_arr = allocator.deref_block(ibuf);
                let o_arr = allocator.deref_block(obuf);
                let weight = self.values[i][o];

                for b in 0..BLOCK_SIZE {
                    o_arr[b] += i_arr[b] * weight;
                }
            }
        }
    }
}

fn truncate_or_expand_fallback(
    allocator: &mut BlockAllocator,
    input_buffers: &mut [&mut AllocatedBlock],
    output_buffers: &mut [&mut AllocatedBlock],
) {
    for (i_dest, o_dest) in input_buffers.iter_mut().zip(output_buffers.iter_mut()) {
        let i_arr = allocator.deref_block(i_dest);
        let o_arr = allocator.deref_block(o_dest);
        for (i_ref, o_ref) in i_arr.iter_mut().zip(o_arr.iter_mut()) {
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
    allocator: &mut BlockAllocator,
    input_format: ChannelFormat,
    output_format: ChannelFormat,
    input_buffers: &mut [&mut AllocatedBlock],
    output_buffers: &mut [&mut AllocatedBlock],
    add_to_outputs: bool,
) {
    if !add_to_outputs {
        for o in output_buffers.iter_mut() {
            allocator.deref_block(o).fill(0.0);
        }
    }

    for i in MIXING_MATRICES.iter() {
        if i.input_format == input_format && i.output_format == output_format {
            i.apply(allocator, input_buffers, output_buffers);
            return;
        }
    }

    truncate_or_expand_fallback(allocator, input_buffers, output_buffers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mono_stereo() {
        let mut allocator = BlockAllocator::new(10);
        let mut ibuf = allocator.allocate_block();
        let mut left = allocator.allocate_block();
        let mut right = allocator.allocate_block();

        allocator.deref_block(&mut ibuf).fill(1.0);

        convert_channels(
            &mut allocator,
            ChannelFormat::Mono,
            ChannelFormat::Stereo,
            &mut [&mut ibuf],
            &mut [&mut left, &mut right],
            false,
        );

        assert_eq!(allocator.deref_block(&mut left), &[1.0f32; BLOCK_SIZE]);
        assert_eq!(allocator.deref_block(&mut right), &[1.0f32; BLOCK_SIZE]);
    }

    #[test]
    fn stereo_to_mono() {
        let mut allocator = BlockAllocator::new(5);
        let mut left = allocator.allocate_block();
        let mut right = allocator.allocate_block();
        let mut out = allocator.allocate_block();

        // They add to 1, so we expect 0.5 out.  We pick these values to be perfect with the mixing matrix: it isn't
        // necessary to use approximate asserts.
        allocator.deref_block(&mut left).fill(0.25);
        allocator.deref_block(&mut right).fill(0.75);

        convert_channels(
            &mut allocator,
            ChannelFormat::Stereo,
            ChannelFormat::Mono,
            &mut [&mut left, &mut right],
            &mut [&mut out],
            false,
        );

        assert_eq!(allocator.deref_block(&mut out), &[0.5f32; BLOCK_SIZE]);
    }

    #[test]
    fn test_truncating() {
        let mut allocator = BlockAllocator::new(5);
        let mut i1 = allocator.allocate_block();
        let mut i2 = allocator.allocate_block();
        let mut i3 = allocator.allocate_block();

        allocator.deref_block(&mut i1).fill(1.0);
        allocator.deref_block(&mut i2).fill(2.0);
        allocator.deref_block(&mut i3).fill(3.0);

        let mut o1 = allocator.allocate_block();
        let mut o2 = allocator.allocate_block();

        // the test here is just the control path: we don't have any untouched buffers to check.
        convert_channels(
            &mut allocator,
            ChannelFormat::new_raw(3),
            ChannelFormat::new_raw(2),
            &mut [&mut i1, &mut i2, &mut i3],
            &mut [&mut o1, &mut o2],
            false,
        );

        assert_eq!(
            allocator.deref_block(&mut i1),
            allocator.deref_block(&mut o1)
        );
        assert_eq!(
            allocator.deref_block(&mut i2),
            allocator.deref_block(&mut o2)
        );
    }

    #[test]
    fn test_expanding() {
        let mut allocator = BlockAllocator::new(5);
        let mut i1 = allocator.allocate_block();
        let mut i2 = allocator.allocate_block();

        allocator.deref_block(&mut i1).fill(1.0);
        allocator.deref_block(&mut i2).fill(2.0);

        let mut o1 = allocator.allocate_block();
        let mut o2 = allocator.allocate_block();
        let mut o3 = allocator.allocate_block();

        allocator.deref_block(&mut o1).fill(0.0);
        allocator.deref_block(&mut o2).fill(0.0);

        // This channel should not be touched, so give it a sentinel value.
        allocator.deref_block(&mut o3).fill(100.0);

        convert_channels(
            &mut allocator,
            ChannelFormat::new_raw(2),
            ChannelFormat::new_raw(3),
            &mut [&mut i1, &mut i2],
            &mut [&mut o1, &mut o2, &mut o3],
            true,
        );

        assert_eq!(
            allocator.deref_block(&mut o1),
            allocator.deref_block(&mut i1)
        );
        assert_eq!(
            allocator.deref_block(&mut o2),
            allocator.deref_block(&mut i2)
        );
        assert_eq!(allocator.deref_block(&mut o3), &[100.0f32; BLOCK_SIZE]);
    }
}
