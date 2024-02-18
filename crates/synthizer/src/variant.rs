/// Declare an enum which can hold any of a fixed number of object types.
///
/// The name comes from C++ std::variant.
///
/// This macro uses paste, so all structs named must be in scope.
///
/// Example:
///
/// ```IGNORE
/// variant!(pub MyStruct, MyThing1, MyThing2);
/// ```
///
/// The generated enum has:
///
/// - A variant for each of the structs of the name `StructNameV`.  These are generally not used directly.
/// - From impls for all contained structs.
/// - `get_payload`, to get the inner payload as a `&dyn Any`.
/// - `get_payload_mut` to get the inner payload mutably.
/// - `TryFrom` impls for all of the contained types.
macro_rules! variant {
    ($v: vis $name: ident, $($structs :ident),*) => {
        paste::paste! {
            $v trait [<$name Payload>]: 'static {
                fn to_variant(self) -> $name;
            }

            #[derive(Debug)]
            $v enum $name {
                $([<$structs V>]($structs)),*
            }

            $(
                impl [<$name Payload>] for $structs {
                    fn to_variant(self) -> $name {
                        $name::[<$structs V>](self)
                    }
                }
            )*

            impl<T> From<T> for $name where
                T: [<$name Payload>],
            {
                fn from(val: T)->$name {
                    val.to_variant()
                }
            }

            impl $name {
                $v fn new(what: impl [<$name Payload>]) -> Self {
                    what.to_variant()
                }

                /// Get the inner data of this variant.
                $v fn get_payload(&self) -> &dyn std::any::Any {
                    match self {
                        $(
                            $name::[<$structs V>](ref x) => x
                        ),*
                    }
                }

                /// Get the inner payload of this enum mutably.
                $v fn get_payload_mut(&mut self) -> &mut dyn std::any::Any {
                    match self {
                        $(
                            $name::[<$structs V>](ref mut x) => x
                        ),*
                    }
                }

                /// Helper method to get a pointer to and the type id of the contained data.
                unsafe fn contained_and_type_id(&mut self) -> (*mut i8, std::any::TypeId) {
                    match self {
                        $(
                            $name::[<$structs V>](x) => {
                                let tid = std::any::TypeId::of::<$structs>();
                                let ptr = x as *mut $structs as *mut i8;
                                (ptr, tid)
                            }
                        ),*
                    }
                }

                /// Call the provided closure on the payload, giving it ownership, if the payload is of the expected
                /// type.  Return `Ok(r)` if te closure was called, else `Err(self)`.
                ///
                /// This is useful for, e.g., command handling: `cmd.take_call::<T>(|x| ...)?;` will bail when the
                /// handler takes the value (alternatively, use `.or_else`).
                $v fn take_call<R, T: [<$name Payload>]>(mut self, closure: impl FnOnce(T) -> R) -> Result<R, Self> {
                    // Careful: self must not move.
                    let (ptr, tid) =  unsafe { self.contained_and_type_id() };

                    if tid == std::any::TypeId::of::<T>() {
                        unsafe{
                            let actual_ptr = ptr as *mut T;
                            let r = closure(actual_ptr.read());
                            // The closure now owns the item. This was a move. Forget self to prevent double drops.
                            #[allow(clippy::forget_non_drop)]
                            std::mem::forget(self);
                            Ok(r)
                        }
                    } else {
                        Err(self)
                    }
                }
            }

            $(
                impl TryFrom<$name> for $structs {
                    type Error = $name;

                    fn try_from(input: $name) -> Result<$structs, Self::Error> {
                        // If this macro is used on single-item enums, we get a warning.
                        #[allow(irrefutable_let_patterns)]
                        if let $name::[<$structs V>](x) = input {
                            Ok(x)
                        } else {
                            Err(input)
                        }
                    }
                }
            )*
        }
    }
}

#[cfg(test)]
mod tests {

    #[derive(Debug)]
    struct TestCmd1;
    #[derive(Debug)]
    struct TestCmd2;

    variant!(TestVariant, TestCmd1, TestCmd2);

    #[test]
    fn test_command_enums() {
        let mut c1 = TestVariant::new(TestCmd1);
        c1.get_payload_mut()
            .downcast_mut::<TestCmd1>()
            .expect("Should be the right command type");

        // Test calling. The first one shouldn't call...
        let mut called = false;
        let c1 = c1
            .take_call(|_x: TestCmd2| {
                called = true;
            })
            .unwrap_err();
        assert!(!called);
        // And the second one should.
        assert!(c1
            .take_call(|_: TestCmd1| {
                called = true;
            })
            .is_ok());
        assert!(called);

        let c2 = TestVariant::new(TestCmd2);
        c2.get_payload()
            .downcast_ref::<TestCmd2>()
            .expect("Should be the right command type");
    }
}
