use crate::v3mc;

use std::fs;
use std::io::Cursor;
use std::error::Error;
use std::path::Path;
use std::path::PathBuf;

pub fn parse_vmesh(vmesh_path:&Path) -> Result<(), Box<dyn Error>> {
    let v3c_contents: Vec<u8> = fs::read(vmesh_path)?;
    println!("Size: {}",v3c_contents.len());

    let v3c_reader = Cursor::new(v3c_contents);
    let v3c_file_header = v3mc::FileHeader::read(v3c_reader)?;
    println!("signature: {}",v3c_file_header.signature);
    //
    Ok(())
}
