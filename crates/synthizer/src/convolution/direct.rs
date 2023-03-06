use supermatch::supermatch_fn;

use crate::maybe_int::*;
use crate::views::*;

/// Evaluate a convolution by directly evaluating the sum.
///
/// Note the important notes on parameter validation below.
///
/// Complexity is `theta(M*N)` where `M` is the impulse length and `N` the input block's length.
///
/// The impulse must be reversed.  The input must start `M - 1` samples in the past and be `M - 1` frames longer than
/// the output.
///
/// This is designed for use in streaming contexts, and so only outputs a block of audio, not the trailing portion.
///
/// # Panics
///
/// Panics if any validation fails, as these are (or should be) programmer bugs.  
#[supermatch_fn]
pub fn convolve_direct(
    input: &(impl ViewMeta<SampleType = f32> + InputView),
    input_channel: usize,
    output: &mut (impl ViewMeta<SampleType = f32> + OutputView),
    output_channel: usize,
    impulse: &[f32],
) {
    assert!(input_channel < input.get_channels());
    assert!(output_channel < output.get_channels());
    assert_eq!(input.get_frames(), output.get_frames() + impulse.len() - 1);
    assert!(!impulse.is_empty());

    let ichans = input.get_channels();
    let ochans = output.get_channels();

    #[supermatch]
    match ichans {
        ichans @ 0..=16usize =>
        {
            #[supermatch]
            match ochans {
                ochans @ 0..=16usize => {
                    convolve_direct_inner(
                        input,
                        FixedInt::<usize, { ichans as i64 }>::new(),
                        input_channel,
                        output,
                        FixedInt::<usize, { ochans as i64 }>::new(),
                        output_channel,
                        impulse,
                    );
                }
                _ => {
                    convolve_direct_inner(
                        input,
                        FixedInt::<usize, { ichans as i64 }>::new(),
                        input_channel,
                        output,
                        VaryingInt::<usize>::new(ochans),
                        output_channel,
                        impulse,
                    );
                }
            }
        }
        _ => {
            convolve_direct_inner(
                input,
                VaryingInt::<usize>::new(ichans),
                input_channel,
                output,
                VaryingInt::<usize>::new(ochans),
                output_channel,
                impulse,
            );
        }
    }
}

/// This function pulls out the values we need in order to be able to diverge with cond_tree.
fn convolve_direct_inner(
    input: &(impl ViewMeta<SampleType = f32> + InputView),
    num_input_channels: impl IntSource<Output = usize>,
    input_channel: usize,
    output: &mut (impl ViewMeta<SampleType = f32> + OutputView),
    num_output_channels: impl IntSource<Output = usize>,
    output_channel: usize,
    impulse: &[f32],
) {
    let num_input_channels = num_input_channels.as_int();
    let num_output_channels = num_output_channels.as_int();

    assert!(input_channel < num_input_channels);
    assert!(output_channel < num_output_channels);
    assert_eq!(input.get_frames(), output.get_frames() + impulse.len() - 1);
    assert!(!impulse.is_empty());

    for frame in 0..output.get_frames() {
        // We could use f64 which would help with precision, but we only use this function on small impulses and using
        // f32 is worth around a 10% performance improvement on average.
        let mut sum: f32 = 0.0;
        for impulse_ind in 0..impulse.len() {
            let impulse_val = unsafe { *impulse.get_unchecked(impulse_ind) };

            let input_frame = frame + impulse_ind;
            let input_ind = num_input_channels * input_frame + input_channel;
            unsafe {
                sum += impulse_val * input.read_index_unchecked(input_ind);
            }
        }

        let output_index = frame * num_output_channels + output_channel;
        unsafe { output.write_index_unchecked(output_index, sum) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Actually 1 2 3 but reversed because that's what the function wants.
    const IMPULSE: [f32; 3] = [3.0, 2.0, 1.0];

    const INPUT: [f32; 7] = [0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
    const EXPECTED: [f32; 5] = [1.0f32, 4.0f32, 10.0f32, 16.0f32, 22.0f32];

    #[test]
    fn test_mono() {
        let mut output = [0.0f32; 5];

        convolve_direct(
            &ImmutableSliceView::new(&INPUT[..], 1),
            0,
            &mut MutableSliceView::<_, false>::new(&mut output[..], 1),
            0,
            &IMPULSE,
        );

        assert_eq!(output, EXPECTED);
    }

    fn gather(slice: &[f32], contained_channels: usize, channel: usize) -> Vec<f32> {
        slice
            .iter()
            .copied()
            .skip(channel)
            .step_by(contained_channels)
            .collect()
    }

    /// turn a slice of 1 channel into a vec of n channels with our 1 channel in the specified channel, and all others set to -INF.
    fn scatter(slice: &[f32], wanted_channels: usize, channel: usize) -> Vec<f32> {
        let mut out = vec![-f32::INFINITY; wanted_channels * slice.len()];
        for (i, v) in slice.iter().copied().enumerate() {
            out[i * wanted_channels + channel] = v;
        }

        out
    }

    struct MultichanTestArgs {
        num_input_channels: usize,
        num_output_channels: usize,
        input_channel: usize,
        output_channel: usize,
    }

    impl MultichanTestArgs {
        fn run(&self) {
            let mut output: Vec<f32> = Vec::new();
            output.resize(EXPECTED.len() * self.num_output_channels, -f32::INFINITY);
            let input = scatter(&INPUT, self.num_input_channels, self.input_channel);
            convolve_direct(
                &ImmutableSliceView::new(&input[..], self.num_input_channels),
                self.input_channel,
                &mut MutableSliceView::<_, false>::new(&mut output[..], self.num_output_channels),
                self.output_channel,
                &IMPULSE,
            );

            let got = gather(&output, self.num_output_channels, self.output_channel);
            assert_eq!(&got, &EXPECTED);
        }
    }

    macro_rules! multichan_test {
        ($num_input_channels: literal, $input_channel: literal, $num_output_channels: literal, $output_channel:literal) => {
            paste::paste! {
                #[test]
                fn [<multichan _ $num_input_channels _ $input_channel _ $num_output_channels _ $output_channel>]() {
                    let args = MultichanTestArgs {
                        num_input_channels: $num_input_channels,
                        num_output_channels: $num_output_channels,
                        input_channel: $input_channel,
                        output_channel: $output_channel,
                    };
                    args.run();
                }
            }
        };
    }

    multichan_test!(2, 0, 2, 0);
    multichan_test!(2, 0, 2, 1);
    multichan_test!(2, 1, 2, 0);
    multichan_test!(2, 1, 2, 1);

    // Now some really weird ones.
    multichan_test!(5, 3, 11, 7);
    multichan_test!(11, 7, 5, 2);
}
