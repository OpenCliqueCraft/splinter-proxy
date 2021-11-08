pub fn position_set(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut data = Vec::from(u8::to_be_bytes(1));
    data.extend(f64::to_be_bytes(x));
    data.extend(f64::to_be_bytes(y));
    data.extend(f64::to_be_bytes(z));
    data
}
