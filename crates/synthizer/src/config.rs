/// The fixed sample rate of the library.
///
/// For efficiency and simplicity, the iunternals use this sample rate and only this sample rate, converting as needed
/// at the edges, or upsampling/downsampling as needed.  If you need more flexibility, e.g. writing a DAW, then consider
/// other crates; this library is intentionally opinionated and is designed for interactive applications in which
/// circumstance high sample rates offer no gain at the cost of increased CPU load.
pub const SR: u16 = 44100;

/// The maximum channels which the library will ever output.
///
/// This serves as a limit for calls to generate audio in the public API and an optimization hint internally to know how
/// many channels to inline.  Going over this channel value for any given node's output in the audio graph will have a
/// negative impact on performance.
///
/// We choose 16 because this is the value needed by third-order ambisonics.  Though we don't support that yet, we may
/// wish to in future, and it makes as good a value as any.
pub(crate) const MAX_CHANNELS: usize = 16;

pub(crate) const MAX_OUTPUTS: usize = 16;
pub(crate) const MAX_INPUTS: usize = 16;

pub(crate) const BLOCK_SIZE: usize = 128;

/// This convenience alias will always be a `[f32; BLOCK_SIZE]`.
///
/// We have unsafe code which depends on this always being an f32 array.
pub(crate) type BlockArray = [f32; BLOCK_SIZE];
