use std::ptr::NonNull;

/// A trait representing a projection from a pointer to another kind of pointer.
///
/// This is like Deref, but for our custom [crate::SharedPtr] type.
///
/// The differences are:
///
/// - The dereferencing operation must go to something which never moves relative to the input pointer, e.g. to a
///   subfield of a larger struct, or an unsizing transformation.
/// - The trait is unsafe, since it's projecting through raw pointers.
///
/// This trait solves the problem that `CoerceUnsized` is nightly, and also that we want to be able to safely define
/// projections which point to subfields.  
///
/// There are some usage patterns:
///
/// - To project to unsized types, impl this trait as `Project<dyn MyTrait>`.  As long as `MyTrait` is defined in the
///   current crate, this allows for direct projections.
/// - To project to unsized types where the orphan rules etc. get in the way, define a helper struct to project to the
///   desired trait, and impl this trait on that.
/// - To project to a subfield, impl this trait on a helper struct which does the subfield projection.
///
/// This crate provides macros to safely implement this trait for the three cases above, as well as a set of impls that
/// cover the standard library.
///
/// # Safety
///
/// If this trait forms a project in which the child (output) pointer has a lifetime less than that of the parent
/// (input), then UB results.
///
/// If this trait does not preserve the `Send + Sync`ness  of the input type, then using [crate::SharedPtr::project] can
/// result in UB by projecting from something `Send + Sync` to something that isn't.
pub unsafe trait Projection {
    type Input: 'static + ?Sized;
    type Output: 'static + ?Sized;

    fn project(input: NonNull<Self::Input>) -> NonNull<Self::Output>;
}

/// Define a projection to a trait from another crate.
///
/// This macro defines a struct which can be passed to [crate::SharedPtr::project] which will convert the pointer to a
/// trait from another crate.  This is useful to avoid the orphan rules.
#[macro_export]
macro_rules! project_trait  {
    ($visibility: vis $struct_name: ident, $trait: path) => {
        $visibility struct $struct_name<T>(std::marker::PhantomData::<*mut T>);

        unsafe impl<T: $trait + Send + Sync + 'static>
        $crate::Projection for $struct_name<T> {
            type Input = T;
            type Output = dyn $trait + Send + Sync + 'static;

            fn project(input: std::ptr::NonNull<Self::Input>)-> std::ptr::NonNull<Self::Output> {
                let raw: *mut dyn  $trait = input.as_ptr();
                unsafe{ std::ptr::NonNull::new_unchecked(raw as *mut (dyn $trait + Send + Sync + 'static)) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::SharedPtr;

    trait TestIndirect {
        fn get_val(&self) -> &str {
            "TestIndirect"
        }
    }

    struct TestStruct(usize);

    impl TestIndirect for TestStruct {}

    project_trait!(IndirectProjection, TestIndirect);

    #[test]
    fn test_trait_projection() {
        let test = TestStruct(5);
        let alloc = crate::Allocator::new(Default::default());

        let ptr = alloc.allocate(test);
        let indirect = SharedPtr::<TestStruct>::project::<
            dyn TestIndirect + Send + Sync + 'static,
            IndirectProjection<TestStruct>,
        >(ptr);
        assert_eq!(indirect.get_val(), "TestIndirect");
    }
}
