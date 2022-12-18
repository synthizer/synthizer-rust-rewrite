/// The fixed sample rate of the library.
///
/// For efficiency and simplicity, the iunternals use this sample rate and only this sample rate, converting as needed
/// at the edges, or upsampling/downsampling as needed.  If you need more flexibility, e.g. writing a DAW, then consider
/// other crates; this library is intentionally opinionated and is designed for interactive applications in which
/// circumstance high sample rates offer no gain at the cost of increased CPU load.
pub const SR: u16 = 44100;
