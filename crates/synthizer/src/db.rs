/// An extension trait for floating point types to convert to and from DB.
///
/// ```IGNORE
/// use synthizer::DbExt;
/// 2.0.db_to_gain();
/// ```
///
/// And so on.
pub trait DbExt {
    fn db_to_gain(self) -> Self;
    fn gain_to_db(self) -> Self;
}

macro_rules! db_impl {
    ($t:ty) => {
        impl DbExt for $t {
            fn db_to_gain(self) -> Self {
                (10.0f64 as $t).powf(self / 20.0)
            }

            fn gain_to_db(self) -> Self {
                20.0 * self.log10()
            }
        }
    };
}

db_impl!(f32);
db_impl!(f64);

#[cfg(test)]
mod tests {
    #[test]
    fn test_conversions() {
        use crate::close_floats::*;

        use super::DbExt;

        close_floats32(0.5f32.gain_to_db(), -6.0, 0.03);
        close_floats64(0.5f64.gain_to_db(), -6.0, 0.03);
        close_floats64((-6.0f64).db_to_gain(), 0.5, 0.03);
        close_floats32((-6.0f32).db_to_gain(), 0.5, 0.03);
    }
}
