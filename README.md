# `jo12bar_os` - x86 OS based on Philipp Oppermann's [`blog_os`][blog_os_link]

Basically just a version of [`blog_os`][blog_os_link] with some personal tweaks, and including some of the "3rd Addition" changes. The goal is to support both UEFI and BIOS booting.

[blog_os_link]: https://os.phil-opp.com/

Nightly Rust is required for building this kernel due to use of some unstable features. See [rust-toolchain.toml](./rust-toolchain.toml).

## Other inspiration
- @Wasabi375's [WasabiOS](https://github.com/Wasabi375/WasabiOS), particularly for the display and testing code.
- @kennystrawnmusic's [CryptOS](https://github.com/kennystrawnmusic/cryptos), particularly for the APIC setup and control code.
