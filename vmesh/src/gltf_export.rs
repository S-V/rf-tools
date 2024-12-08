use crate::v3mc;

use std::fs;
use std::io::Cursor;
use std::error::Error;
use std::path::Path;
use std::path::PathBuf;

use binrw::BinReaderExt;
use binrw::{
    binrw,    // #[binrw] attribute
    BinRead,  // trait for reading
    BinWrite, // trait for writing
};

pub fn parse_vmesh(vmesh_path:&Path) -> Result<(), Box<dyn Error>> {
    let v3c_contents: Vec<u8> = fs::read(vmesh_path)?;
    println!("Size: {}",v3c_contents.len());

    let mut v3c_reader = Cursor::new(v3c_contents);

    let v3c_file_header: v3mc::FileHeader = v3c_reader.read_le()?;
    println!("v3c_file_header: {:?}",v3c_file_header);

    //
    Ok(())
}
