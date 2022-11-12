# synthizer-rust-rewrite

This is a tentative prototype of rewriting Synthizer in Rust.

## Why?

Current Synthizer has a lot of problems:

- Bad/no tests.
- memory safety issues, becuase it's in C++
- Not reusable or orthogonal. Lots of entangled components.
- No SIMD dispatching, etc.

I was going to fix the C++ because Rust doesn't have:

- Const generics which support doing compile-time math (that is, you can make an array of `N` items, but not of `N *
  ANOTHER_CONST`).
  - Fixed by the `generic_array` crate.
- variadic generics (for vbool, etc).
  - We can do that with macros.
- Fast/zero overhead indexing without unsafe in all contexts.
  - Also fixed by `generic_array`.
- SIMD.
  - Fixed by just waiting: we now have cpuid, multiversioning, and the intrinsics, but we didn't originally.
- An object system supporting inheritance
  - Synthizer only has two or three cases where this really matters.  We can cook something up even if it's a bit ugly.
    At the original writing, I thought this would be a bigger deal.
  - Also Synthizer is using it for allocation tricks, but I am already planning to basically rewrite `shared_ptr` so
    whatever.
- No allocator API
  - Not actually fixed. This is the one big one we can't completely overcome.
  - We can get close though: we can always pull a `T` from our own allocators, and as long as our own types understand
    them we can make that work out.
  - We can defer our own types to a background thread for destruction, and then when they drop they also drop their
    fields.  Since we're planning to have a custom `Arc` already, that's fine enough.

Now the obvious solution is "just add some Rust to the C++ and slowly move over" which doesn't work.  You can't reliably
link multiple static libraries that contain Rust code to each other.  That kills static linking and, importantly, static
linking in the synthizer Rust crate where we have Rust linking to C++ which links to other Rust possibly on a different
compiler version.  But if we just bite the bullet we can probably port the old Synthizer over pretty fast because all
the problems are solved, either in the code or in my head as things I'm going to do to the code.  Fixing the C++ was
also going to be a big refactor in a way which would have basically been a rewrite anyway, so let's see how fast just
using Rust gets, then rewrite the C API on top.
