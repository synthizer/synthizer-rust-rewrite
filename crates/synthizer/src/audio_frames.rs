use crate::core_traits::*;

impl AudioFrame<f64> for f64 {
    fn default_frame() -> Self {
        0.0f64
    }

    fn channel_count(&self) -> usize {
        1
    }

    fn get(&self, index: usize) -> &f64 {
        debug_assert_eq!(index, 0);
        self
    }

    fn get_mut(&mut self, index: usize) -> &mut f64 {
        debug_assert_eq!(index, 0);
        self
    }

    fn set(&mut self, index: usize, value: f64) {
        debug_assert_eq!(index, 0);
        *self = value;
    }
}

impl<T, const N: usize> AudioFrame<T> for [T; N]
where
    T: Copy + Default,
{
    fn default_frame() -> Self {
        [(); N].map(|_| Default::default())
    }

    fn channel_count(&self) -> usize {
        N
    }

    fn get(&self, index: usize) -> &T {
        &self[index]
    }

    fn get_mut(&mut self, index: usize) -> &mut T {
        &mut self[index]
    }

    fn set(&mut self, index: usize, value: T) {
        self[index] = value;
    }
}

macro_rules! impl_tuple {
    ($(($t:ident, $i: tt)),*,) => {
        impl<Elem, $($t),*> AudioFrame<Elem> for ($($t,)*) where
        $($t: AudioFrame<Elem>,)* Elem: Copy + Default,
        {
            fn default_frame() -> Self {
                ($($t::default_frame(),)*)
            }

            fn channel_count(&self) -> usize {
                0 $(+ self.$i.channel_count())*
            }

            fn get(&self, index: usize) -> &Elem {
                let mut index = index;
                $(
                    if index < self.$i.channel_count() {
                        return &self.$i.get(index);
                    }
                    #[allow(unused_assignments)] {
                        index -= self.$i.channel_count();
                    }
                )*
                panic!("Index out of bounds");
            }

            fn get_mut(&mut self, mut index: usize) -> &mut Elem {
                $(
                    if index < self.$i.channel_count() {
                        return self.$i.get_mut(index);
                    }
                    #[allow(unused_assignments)] {
                        index -= self.$i.channel_count();
                    }
                )*
                panic!("Index out of bounds");
            }

            fn set(&mut self, index: usize, value: Elem) {
                let mut index = index;
                $(
                    if index < self.$i.channel_count() {
                        self.$i.set(index, value);
                        return;
                    }
                    #[allow(unused_assignments)] {
                        index -= self.$i.channel_count();
                    }
                )*
                panic!("Index out of bounds");
            }
        }
    };
}

macro_rules! repl_tuple {
    ($count: tt) => {
        seq_macro::seq!(N in 0..$count {
            impl_tuple!(#((T~N, N),)*);
        });
    }
}

seq_macro::seq!(N in 1..8 {
    repl_tuple!(N);
});

impl<T> AudioFrame<T> for ()
where
    T: Copy + Default,
{
    fn default_frame() -> Self {}

    fn channel_count(&self) -> usize {
        0
    }

    fn get(&self, _: usize) -> &T {
        panic!("Index out of bounds");
    }

    fn get_mut(&mut self, _index: usize) -> &mut T {
        panic!("Index out of bounds");
    }

    fn set(&mut self, _: usize, _: T) {
        panic!("Index out of bounds");
    }
}
