#![no_std]
#![no_main]
#![feature(lang_items)]
#![feature(alloc_error_handler)]
#![feature(panic_info_message)]
#![allow(non_snake_case)]

use ckb_std::high_level::{load_cell_type, load_script};
use ckb_std::{
    ckb_constants::Source,
    ckb_types::{bytes::Bytes, prelude::*},
    debug, default_alloc, entry,
    error::SysError,
};
use core::result::Result;
entry!(entry);
default_alloc!();

/// Program entry
fn entry() -> i8 {
    // Call main function and return error code
    match main() {
        Ok(_) => 0,
        Err(err) => err as i8,
    }
}

/// Error
#[repr(i8)]
pub enum Error {
    IndexOutOfBound = 1,
    ItemMissing,
    LengthNotEnough,
    Encoding,
    // Add customized errors here...
    InvalidToCKBCell,
}

impl From<SysError> for Error {
    fn from(err: SysError) -> Self {
        use SysError::*;
        match err {
            IndexOutOfBound => Self::IndexOutOfBound,
            ItemMissing => Self::ItemMissing,
            LengthNotEnough(_) => Self::LengthNotEnough,
            Encoding => Self::Encoding,
            Unknown(err_code) => panic!("unexpected sys error {}", err_code),
        }
    }
}

fn main() -> Result<(), Error> {
    verify()
}

fn verify() -> Result<(), Error> {
    let args: Bytes = load_script()?.args().unpack();
    verify_toCKB_cells(Source::GroupInput, args.as_ref())?;
    Ok(())
}

fn verify_toCKB_cells(source: Source, args: &[u8]) -> Result<(), Error> {
    for i in 0.. {
        match load_cell_type(i, source) {
            Ok(type_script_opt) => {
                if type_script_opt.is_none()
                    || type_script_opt.unwrap().as_slice()[0..54] != args[..]
                {
                    debug!("{:?}-{}: the cell is not toCKB type", source, i);
                    continue;
                }
            }
            Err(SysError::IndexOutOfBound) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        debug!("{:?}-{}: the cell is valid toCKB cell", source, i);
    }

    Ok(())
}
