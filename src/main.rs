#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

use std::{env, fs};

fn main() {
    let current_exe = env::current_exe().unwrap();
    let uefi_target = current_exe.with_file_name("uefi.img");
    let bios_target = current_exe.with_file_name("bios.img");

    fs::copy(env!("UEFI_IMAGE"), &uefi_target).expect("couldn't copy uefi image to target dir");
    fs::copy(env!("BIOS_IMAGE"), &bios_target).expect("couldn't copy bios image to target dir");

    println!("UEFI disk image at {}", uefi_target.display());
    println!("BIOS disk image at {}", bios_target.display());
}
