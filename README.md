# splitfile
File i/o across volumes.

This module contains methods designed to mirror and be used in place of fs::OpenOptions and fs::File, that while providing the interace of a single file, read and write data across one or more volumes of a specified maximum size.  

Example use cases include using SplitFile as a reader/writer in conjunction with crates such as tar, zip, rust-lzma, etc.

# Links

[Crates.io](https://crates.io/crates/splitfile)
[Docs.rs](https://docs.rs/splitfile)
