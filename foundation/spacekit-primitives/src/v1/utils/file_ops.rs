use std::fs::File;
use std::io::{Read, Write};

pub fn save_to_file(filename: &str, data: &[u8]) -> std::io::Result<()> {
    let mut file = File::create(filename)?;
    file.write_all(data)?;
    Ok(())
}

pub fn load_from_file(filename: &str) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut file = File::open(filename)?;
    file.read_to_end(&mut data)?;
    Ok(data)
}
