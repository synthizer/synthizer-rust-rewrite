use supermatch::supermatch_fn;

#[test]
fn test_const_ranges_suffix_on_left() {
    #[supermatch_fn]
    fn inner(x: i32) -> i32 {
        #[supermatch]
        match x {
            x @ 0i32..=15 => {
                const B: i32 = x;
                B
            }
            16 => 16i32,
            _ => i32::MAX,
        }
    }

    let got = (0..=17).map(inner).collect::<Vec<_>>();
    assert_eq!(
        got,
        vec![
            0,
            1,
            2,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
            10,
            11,
            12,
            13,
            14,
            15,
            16,
            i32::MAX
        ]
    );
}

#[test]
fn test_const_ranges_suffix_on_right() {
    #[supermatch_fn]
    fn inner(x: i32) -> i32 {
        #[supermatch]
        match x {
            x @ 0..=15i32 => {
                const B: i32 = x;
                B
            }
            16 => 16i32,
            _ => i32::MAX,
        }
    }

    let got = (0..=17).map(inner).collect::<Vec<_>>();
    assert_eq!(
        got,
        vec![
            0,
            1,
            2,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
            10,
            11,
            12,
            13,
            14,
            15,
            16,
            i32::MAX
        ]
    );
}

#[test]
fn test_const_ranges_suffix_negatives() {
    #[supermatch_fn]
    fn inner(x: i32) -> i32 {
        #[supermatch]
        match x {
            x @ -5..=5i32 => {
                const B: i32 = x;
                B
            }
            6 => 6i32,
            _ => i32::MAX,
        }
    }

    let got = (-5..=7).map(inner).collect::<Vec<_>>();
    assert_eq!(
        got,
        vec![-5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, i32::MAX,]
    );
}

#[test]
fn test_or_patterns() {
    enum Displayable {
        Int(u32),
        String(&'static str),
    }

    #[supermatch_fn]
    fn inner(d: Displayable) -> String {
        #[supermatch]
        match d {
            Displayable::Int(x) | Displayable::String(x) => x.to_string(),
        }
    }

    assert_eq!(inner(Displayable::Int(5)), "5".to_string());
    assert_eq!(inner(Displayable::String("hello")), "hello".to_string());
}

#[test]
fn test_nested() {
    #[supermatch_fn]
    fn inner(x: i32, y: i32) -> (i32, i32) {
        #[supermatch]
        match x {
            x @ 0..16i32 => {
                const B1: i32 = x;
                #[supermatch]
                match y {
                    y @ 0..16i32 => {
                        const B2: i32 = y;
                        (B1, B2)
                    }
                    _ => panic!("Unreachable"),
                }
            }

            _ => panic!("Unreachable"),
        }
    }

    assert_eq!(inner(2, 3), (2, 3));
}
