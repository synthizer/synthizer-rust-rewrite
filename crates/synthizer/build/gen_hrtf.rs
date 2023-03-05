use itertools::Itertools;
use prost::Message;
use synthizer_protos::hrtf as p_hrtf;

/// because is_sorted isn't stable yet.
fn check_sorted<T, K: std::cmp::PartialOrd>(slice: &[T], mut by_fn: impl FnMut(&T) -> K) -> bool {
    if slice.is_empty() {
        return true;
    }

    let mut prev = by_fn(&slice[0]);
    for i in slice.iter().take(1) {
        let cur = by_fn(i);
        if cur < prev {
            return false;
        }
        prev = cur;
    }

    true
}

fn parse_one(name: &str) -> p_hrtf::HrtfDataset {
    use std::io::Read;

    let mut file = std::fs::File::open(format!("src/datasets/bin_protos/{}.bin", name)).unwrap();
    let mut all_bytes = vec![];
    file.read_to_end(&mut all_bytes).unwrap();

    p_hrtf::HrtfDataset::decode(&all_bytes[..]).unwrap()
}

fn validate(db: &p_hrtf::HrtfDataset, name: &str) {
    if db.elevations.is_empty() {
        panic!("Found an HRTF dataset without any elevations: for {}", name);
    }

    let expected_len = db.elevations[0]
        .azimuths
        .get(0)
        .expect("There must be at least one azimuth in each elevation")
        .impulse
        .len();

    if expected_len == 0 {
        panic!(
            "Dataset {}: the azimuth impulse lengthy must not be 0",
            expected_len
        );
    }

    if !check_sorted(&db.elevations[..], |x| x.angle) {
        panic!("{}: elevations must be sorted", name);
    }

    for (i, elev) in db.elevations.iter().enumerate() {
        if elev.angle < -90.0 || elev.angle > 90.0 {
            panic!(
                "Elevation {} of {} has angle {}, but must be between -90 and 90",
                i, name, elev.angle
            );
        }

        if !check_sorted(&elev.azimuths[..], |x| x.angle) {
            panic!("Hrtf dataset {}: elev {}: azimuths are not sorted", name, i);
        }

        for (az_i, az) in elev.azimuths.iter().enumerate() {
            if az.angle < 0.0 || az.angle > 360.0 {
                panic!(
                    "{}: azimuth {}: has angle {} which is not in 0.0..=360.0",
                    name, az_i, az.angle
                );
            }

            if az.impulse.len() != expected_len {
                panic!(
                    "{}: azimuth {} of elevation {}: found unexpected length {}",
                    name,
                    az_i,
                    i,
                    az.impulse.len()
                );
            }
        }
    }
}

fn az_lit(azimuth: &p_hrtf::HrtfAzimuth) -> String {
    let impulse_lit = azimuth.impulse.iter().join(", ");
    let impulse_lit = format!("Cow::Borrowed(&[{impulse_lit}].as_slice())");
    let angle = format!("{:0.1}", azimuth.angle);
    format!("HrtfAzimuth {{ angle: {angle}, impulse: {impulse_lit} }}")
}

fn elev_lit(elev: &p_hrtf::HrtfElevation) -> String {
    let az_lits = elev.azimuths.iter().map(az_lit).join(",\n");

    let az_lits = format!("Cow::Borrowed(&[{az_lits}].as_slice())");
    let angle = format!("{:0.1}", elev.angle);
    format!("HrtfElevation {{ angle: {angle}, azimuths: {az_lits} }}")
}

fn dataset_lit(db: &p_hrtf::HrtfDataset) -> String {
    let elev_lits = db.elevations.iter().map(elev_lit).join(",\n");
    let elev_lits = format!("Cow::Borrowed(&[{elev_lits}].as_slice())");
    format!("HrtfDataset {{ elevations: {elev_lits} }}")
}

/// Returns a code fragment to define one dataset.
fn handle_one(name: &str) -> String {
    let db = parse_one(name);
    validate(&db, name);

    let upper = name.to_uppercase();
    let lit = dataset_lit(&db);
    format!("pub static {upper}: HrtfDataset<'static> = {lit};")
}

pub fn gen_hrtf() {
    use std::io::Write;

    let prelude = r#"
    use crate::hrtf::*;
    "#;

    let fragment = handle_one("mit_kemar");

    let defs = format!(
        r#"
    {prelude}
    {fragment}
    "#
    );

    let out_path = format!("{}/mit_kemar.rs", std::env::var("OUT_DIR").unwrap());

    {
        let mut output = std::fs::File::create(&out_path).unwrap();
        output.write_all(defs.as_bytes()).unwrap();
    }

    // It is almost impossible to debug compilation errors if this file isn't formatted.
    std::process::Command::new("rustfmt")
        .arg(out_path)
        .output()
        .unwrap();
}
