# `jo12bar_os` - Embedded ARM OS based on Philipp Oppermann's [`blog_os`][blog_os_link]

Basically just a version of [`blog_os`][blog_os_link] with some personal tweaks.

[blog_os_link]: https://os.phil-opp.com/

Nightly Rust is required for building this kernel due to use of some unstable features. So, make sure to tell rustup to use nightly:

```shell
rustup override set nightly
```
