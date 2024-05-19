use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

impl Cli {
    pub fn command(&self) -> Commands {
        self.command.clone().unwrap_or(Commands::Run {
            boot_mode: BootMode::Uefi,
        })
    }
}

#[derive(Clone, Debug, Subcommand)]
pub enum Commands {
    Run {
        #[arg(value_enum, default_value_t = BootMode::Uefi)]
        boot_mode: BootMode,
    },

    CopyDiskImages,
}

#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq, ValueEnum)]
pub enum BootMode {
    Uefi,
    Bios,
}
