use std::borrow::Cow;

/// A definition of an HRTF dataset.
#[derive(Debug)]
pub struct HrtfDataset<'a> {
    /// the elevations in this dataset, sorted from least to greatest.
    pub elevations: Cow<'a, &'a [HrtfElevation<'a>]>,
}

#[derive(Debug)]
pub struct HrtfElevation<'a> {
    /// The angle of this elevation angle in degrees where -90 is straight down and 90 striaght up.
    ///
    /// This slightly odd definition matches the literature and all of the HRTF datasets therein.
    pub angle: f64,

    /// the azimuths in this elevation sorted clockwise starting from 0.
    pub azimuths: Cow<'a, &'a [HrtfAzimuth<'a>]>,
}

#[derive(Debug)]
pub struct HrtfAzimuth<'a> {
    /// The angle of this azimuth in degrees starting from 0 and proceeding clockwise.
    ///
    /// We use degrees because every HRTF dataset in the literature uses degrees.
    pub angle: f64,
    pub impulse: Cow<'a, &'a [f32]>,
}
