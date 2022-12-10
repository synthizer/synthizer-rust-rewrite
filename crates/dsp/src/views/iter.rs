use super::*;

/// An iterator over an input view which uses the index to iterate over all samples, one by one.
pub struct ViewIter<'a, T: InputView + ViewMeta> {
    pub(crate) view: &'a T,
    pub(crate) index: usize,
}

impl<'a, T: InputView + ViewMeta> Iterator for ViewIter<'a, T> {
    type Item = T::SampleType;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.view.get_len() {
            None
        } else {
            let o = self.view.read_sample(self.index);
            self.index += 1;
            Some(o)
        }
    }
}
