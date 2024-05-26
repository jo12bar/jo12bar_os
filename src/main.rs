//! # jo12bar_os runner
//!
//! Takes care of running jo12bar_os inside of QEMU, running unit tests, or running
//! other utilities.

#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

mod cli;

use std::{
    env, fs,
    process::{self, Command},
};

use clap::Parser;
use color_eyre::eyre::Context;

fn main() -> color_eyre::Result<()> {
    let cli = cli::Cli::parse();

    match cli.command() {
        cli::Commands::Run { boot_mode } => match boot_mode {
            cli::BootMode::Uefi => run_qemu_uefi()?,
            cli::BootMode::Bios => run_qemu_bios()?,
        },
        cli::Commands::CopyDiskImages => copy_disk_images_to_exe_location()?,
    }

    Ok(())
}

fn run_qemu_uefi() -> color_eyre::Result<()> {
    let mut qemu = Command::new("qemu-system-x86_64");
    qemu.arg("-drive");
    qemu.arg(format!("format=raw,file={}", env!("UEFI_IMAGE")));
    qemu.arg("-bios").arg(ovmf_prebuilt::ovmf_pure_efi());
    qemu.arg("-device");
    qemu.arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    qemu.arg("-serial");
    qemu.arg("stdio");
    let exit_status = qemu.status()?;
    process::exit(exit_status.code().unwrap_or(-1));
}

fn run_qemu_bios() -> color_eyre::Result<()> {
    let mut qemu = Command::new("qemu-system-x86_64");
    qemu.arg("-drive");
    qemu.arg(format!("format=raw,file={}", env!("BIOS_IMAGE")));
    qemu.arg("-device");
    qemu.arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    qemu.arg("-serial");
    qemu.arg("stdio");
    let exit_status = qemu.status()?;
    process::exit(exit_status.code().unwrap_or(-1));
}

fn copy_disk_images_to_exe_location() -> color_eyre::Result<()> {
    let current_exe = env::current_exe()?;
    let uefi_target = current_exe.with_file_name("uefi.img");
    let bios_target = current_exe.with_file_name("bios.img");

    fs::copy(env!("UEFI_IMAGE"), &uefi_target)
        .wrap_err("couldn't copy uefi image to target dir")?;
    fs::copy(env!("BIOS_IMAGE"), &bios_target)
        .wrap_err("couldn't copy bios image to target dir")?;

    println!("UEFI disk image at {}", uefi_target.display());
    println!("BIOS disk image at {}", bios_target.display());

    Ok(())
}
