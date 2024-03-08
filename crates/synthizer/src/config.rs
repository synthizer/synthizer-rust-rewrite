/// The fixed sample rate of the library.
///
/// For efficiency and simplicity, the iunternals use this sample rate and only this sample rate, converting as needed
/// at the edges, or upsampling/downsampling as needed.  If you need more flexibility, e.g. writing a DAW, then consider
/// other crates; this library is intentionally opinionated and is designed for interactive applications in which
/// circumstance high sample rates offer no gain at the cost of increased CPU load.
pub const SR: u32 = 44100;

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

/// The number of samples between server parameter updates
///
/// In order to be efficient, Synthizer only reacts to user-specified changes every `BLOCK_SIZE` frames.  We call this a
/// block.  Avoiding trying to respond on every sample allows for amortizing things such as graph updates internally.
///
/// When writing code which uses Synthizer to synthesize audio instead of outputting to an audio device, it is useful to
/// know this value.  In particular, it is possible to use it to declare arrays (e.g. intermediarte buffers as struct
/// fields) rather than heap allocations and to make simulation updates happen exactly on block boundaries.  When
/// synthesizing audio, note that the performance characteristics don't change based on how many samples you ask for.
/// Internally, partial blocks are synthesized as whole blocks and then streamed to the application.
///
/// Changing the value of this constant is *not* considered a breaking change to the public API.  It is exposed to allow
/// for things like `[f32; BLOCK_SIZE]`.  The guarantee we make here is that it will never be raised such that using
/// such arrays in TLS or statics would cause an issue.  Synthizer itself frequently uses TLS to store such arrays. Note
/// that the stack was not mentioned; even today, stereo arrays on the stack are impractically large, especially in
/// debug builds.
pub const BLOCK_SIZE: usize = 128;

/// This convenience alias will always be a `[f64; BLOCK_SIZE]`.
///
/// We have unsafe code which depends on this always being a float array.
pub(crate) type BlockArray = [f64; BLOCK_SIZE];
