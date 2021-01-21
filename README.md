# splitfile
File i/o across volumes.

This module provides the same interface as fs::OpenOptions and fs::File, but reads and writes data across one or more file volumes of a specified maximum size.  

Example use cases include using SplitFile as a reader/writer in conjunction with crates such as tar, zip, rust-lzma, etc.

### Links

* Crate on [crates.io](https://crates.io/crates/splitfile)
* Documentation on [docs.rs](https://docs.rs/splitfile)
