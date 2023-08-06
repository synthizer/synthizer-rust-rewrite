/// An implementation of recycling for thingbuf which works with `Option`.
///
/// This just sets Option to None.  It's suitable for our common case wherein we have queues that just need empty slots without resource reuse.
#[derive(Default)]
pub(crate) struct OptionRecycler;

impl<T> thingbuf::recycling::Recycle<Option<T>> for OptionRecycler {
    fn new_element(&self) -> Option<T> {
        None
    }

    fn recycle(&self, element: &mut Option<T>) {
        *element = None;
    }
}
