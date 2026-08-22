use alloy_primitives::Address;
use std::error::Error;
use std::str::FromStr;

// std
use std::fs::File;
use std::io;
use std::io::{Read, Write};

pub mod file_ops;

pub mod network_metrics;

/// Save a public or secret key to file
pub fn save_key_to_file(key_data: &str, file_path: &str) -> Result<(), std::io::Error> {
    std::fs::write(file_path, key_data)?;
    Ok(())
}

/// Reads a file and returns its contents as a hex string.
pub fn read_hex_from_file(file_path: &str) -> io::Result<String> {
    let mut file = File::open(file_path)?;
    let mut hex_string = String::new();
    file.read_to_string(&mut hex_string)?;
    Ok(hex_string)
}

/// Reads a file and returns its contents as a plain string (for Base58 and other encodings)
pub fn read_from_file(file_path: &str) -> io::Result<String> {
    let mut file = File::open(file_path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content.trim().to_string())
}

pub fn str_to_address(address: &str) -> Result<Address, Box<dyn Error>> {
    Address::from_str(address).map_err(|e| Box::new(e) as Box<dyn Error>)
}
