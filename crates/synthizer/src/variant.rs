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

            $v trait [<$name Payload>]<E> {
                fn to_variant(self) -> E;
            }

            $v enum $name {
                $([<$structs V>]($structs)),*
            }

            $(
                impl [<$name Payload>]<$name> for $structs {
                    fn to_variant(self) -> $name {
                        $name::[<$structs V>](self)
                    }
                }
            )*

            impl<T> From<T> for $name where
                T: [<$name Payload>]<$name>,
            {
                fn from(val: T)->$name {
                    val.to_variant()
                }
            }

            impl $name {
                $v fn new(what: impl [<$name Payload>]<Self>) -> Self {
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
    use super::*;

    struct TestCmd1;
    struct TestCmd2;

    variant!(TestVariant, TestCmd1, TestCmd2);

    #[test]
    fn test_command_enums() {
        let mut c1 = TestVariant::new(TestCmd1);
        c1.get_payload_mut()
            .downcast_mut::<TestCmd1>()
            .expect("Should be the right command type");

        let c2 = TestVariant::new(TestCmd2);
        c2.get_payload()
            .downcast_ref::<TestCmd2>()
            .expect("Should be the right command type");
    }
}
