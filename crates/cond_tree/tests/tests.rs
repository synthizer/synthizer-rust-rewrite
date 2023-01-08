use cond_tree::*;

#[test]
#[diverge_fn]
fn bools() {
    for a in [false, true] {
        for b in [false, true] {
            let got_a;
            let got_b;

            #[diverge(
                let                 cond_a = a,
                let cond_b = b,
            )]
            {
                got_a = Some(cond_a.get());
                got_b = Some(cond_b.get());
            };

            assert_eq!(got_a, Some(a));
            assert_eq!(got_b, Some(b));
        }
    }
}

#[test]
#[diverge_fn]
fn direct_idents() {
    for a in [false, true] {
        for b in [false, true] {
            let got_a;
            let got_b;

            #[diverge(a, b)]
            {
                got_a = Some(a.get());
                got_b = Some(b.get());
            };

            assert_eq!(got_a, Some(a));
            assert_eq!(got_b, Some(b));
        }
    }
}

#[test]
#[diverge_fn]
fn using_result() {
    let got;

    #[diverge(
        let a: (u32, u32) = Ok(5),
        let b: (u32, u32) = Err(10),
    )]
    {
        got = a * b;
    };

    assert_eq!(got, 50);
}

#[test]
#[diverge_fn]
fn using_diverging_consts() {
    let mut got = vec![];

    for c in [true, false] {
        #[diverge(
            const A: (u32, &'static str) = if c { 5 } else { "foo" },
        )]
        {
            got.push(A.to_string());
        };
    }

    assert_eq!(got, vec!["5".to_string(), "foo".to_string()]);
}

#[test]
#[diverge_fn]
fn consts_are_actually_consts() {
    #[diverge(
        const A: (usize, usize) = if true { 5 } else { 10 },
        const B:(usize, usize) = if true { 5 } else { 10 },
    )]
    {
        // The test is that using a constant here builds.
        #[allow(dead_code)]
        const C: [[u32; A]; B] = [[0; A]; B];
    }
}

#[test]
#[diverge_fn]
fn slow_and_fast_are_not_inverted() {
    #[diverge(
        let a: (usize, &'static str) = if true { 5 } else { "foo" },
        let b: (usize, &'static str) = if false { 10 } else { "bar" },
    )]
    {
        assert_eq!(a.to_string(), "5".to_string());
        assert_eq!(b.to_string(), "bar".to_string());
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct CustomTrueTy;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct CustomFalseTy;

impl From<CustomTrueTy> for CustomFalseTy {
    fn from(_: CustomTrueTy) -> Self {
        CustomFalseTy
    }
}

struct CustomDiv(bool);

impl Divergence for CustomDiv {
    type Slow = CustomFalseTy;
    type Fast = CustomTrueTy;

    fn evaluate_divergence(self) -> Cond<Self::Fast, Self::Slow> {
        if self.0 {
            Cond::Fast(CustomTrueTy)
        } else {
            Cond::Slow(CustomFalseTy)
        }
    }
}

#[test]
#[diverge_fn]
fn tuple_collapsing() {
    assert_eq!(
        (CustomDiv(true), CustomDiv(true), CustomDiv(true)).evaluate_divergence(),
        Cond::Fast((CustomTrueTy, CustomTrueTy, CustomTrueTy))
    );

    assert_eq!(
        (CustomDiv(true), CustomDiv(false), CustomDiv(true)).evaluate_divergence(),
        Cond::Slow((CustomFalseTy, CustomFalseTy, CustomFalseTy))
    );
}

#[test]
#[diverge_fn]
fn test_maybe_int() {
    let is_matching: MaybeInt<u16, 12> = MaybeInt::new(12);
    let is_mismatching: MaybeInt<u16, 14> = MaybeInt::new(12);

    #[diverge(is_matching, is_mismatching)]
    {
        assert!(is_matching.is_fixed());
        assert!(!is_mismatching.is_fixed());
    }
}

#[diverge_fn]
#[test]
fn nested_diverging() {
    let mut got = vec![];

    for i in [false, true] {
        for j in [false, true] {
            #[diverge(
                let x = if i { 5 } else { 4 }
            )]
            {
                #[diverge(
                    let y = if j { 8 } else { 7 }
                )]
                {
                    got.push((x, y));
                };
            }
        }
    }

    assert_eq!(got, vec![(4, 7), (4, 8), (5, 7), (5, 8)]);
}
