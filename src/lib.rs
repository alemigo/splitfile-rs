#![warn(missing_docs)]
//! File i/o across volumes.
//!
//! This module contains methods designed to mirror and be used in place
//! of `fs::OpenOptions` and `fs::File`, that while providing the interace
//! of a single file, read and write data across one or more volumes of a
//! specified maximum size.
//!
//! Example use cases include using `splitfile` as a reader/writer in
//! conjunction with crates such as tar, zip, rust-lzma, etc.
//!

use std::cmp;
use std::ffi::OsString;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions as OpenOptionsFs;
use std::io::{Error, ErrorKind, Read, Result, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Options and flags which can be used to configure how a file is opened.
///
/// This builder mirrors the usage and options of `fs::OpenOptions`, and
/// returns a `SplitFile` instance.
#[derive(Clone, Debug, Default)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}

/// A reference to an open set of volumes on the filesystem.
///
/// An instance of SplitFile can be read and/or written in the same way as a
/// single file is via `fs::File`, but with data allocated
/// across volumes.
///
/// Second and subsequent volumes written use the path and filename of the
/// first volume, and append the extension ".n", where n is the index of each
/// respective volume.
///
/// SplitFile implements Read, Write and Seek traits.
#[derive(Debug)]
pub struct SplitFile {
    volumes: Vec<Volume>,
    path: PathBuf,
    opts: OpenOptions,
    volsize: u64,
    index: usize,
    first_open: bool,
}

#[derive(Debug)]
struct Volume {
    file: File,
    pos: u64,
    reset: bool,
}

#[derive(Debug)]
struct Filenames {
    path: PathBuf,
    index: usize,
}

impl Iterator for Filenames {
    type Item = PathBuf;

    fn next(&mut self) -> Option<Self::Item> {
        self.index += 1;
        Some(Filenames::by_index(self.path.clone(), self.index))
    }
}

impl Filenames {
    fn new(path: PathBuf, start_index: usize) -> Filenames {
        Filenames {
            path,
            index: start_index - 1,
        }
    }

    fn by_index(path: PathBuf, index: usize) -> PathBuf {
        if index == 1 {
            path
        } else {
            let mut os = path.into_os_string();
            os.push(OsString::from(format!(".{}", index.to_string())));
            PathBuf::from(os)
        }
    }
}

impl OpenOptions {
    /// Creates a blank set of options ready for configuartion.
    ///
    /// All options are initially set to `false`.
    pub fn new() -> OpenOptions {
        Default::default()
    }

    /// Sets the option for read access.
    ///
    /// This option, when true, will indicate that the file should be
    /// `read`-able if opened.
    pub fn read(&mut self, read: bool) -> &mut OpenOptions {
        self.read = read;
        self
    }

    /// Sets the option for write access.
    ///
    /// This option, when true, will indicate that the file should be
    /// `write`-able if opened.
    pub fn write(&mut self, write: bool) -> &mut OpenOptions {
        self.write = write;
        self
    }

    /// Sets the option for append mode.
    ///
    /// This option, when true, means that writes will append to a file instead
    /// of overwriting previous contents.
    pub fn append(&mut self, append: bool) -> &mut OpenOptions {
        self.append = append;
        self
    }

    /// Sets the option for trncating a previous file.  This truncate the first
    /// volume, and delete all additional volume files using `fs::remove_file`.
    pub fn truncate(&mut self, truncate: bool) -> &mut OpenOptions {
        self.truncate = truncate;
        self
    }

    /// Sets the option for creating a new file.
    ///
    /// This option indicates whether a new file will be created if the file
    /// does not yet already exist.
    ///
    /// In order for the file to be created, [`write`] or [`append`] access must
    /// be used.
    ///
    /// [`write`]: #method.write
    /// [`append`]: #method.append
    pub fn create(&mut self, create: bool) -> &mut OpenOptions {
        self.create = create;
        self
    }

    /// Sets the option to always create a new file.
    ///
    /// This option indicates whether a new file will be created.
    /// No file is allowed to exist at the target location, also no (dangling)
    /// symlink.
    pub fn create_new(&mut self, create_new: bool) -> &mut OpenOptions {
        self.create_new = create_new;
        self
    }

    /// Opens a file at `path` with the options specified by `self`.  Path refers
    /// to the path of the first volume.  Volsize is the maximum size of each
    /// volume.
    pub fn open<P: AsRef<Path>>(&self, path: P, volsize: u64) -> Result<SplitFile> {
        self._open(path.as_ref(), volsize)
    }

    fn _open(&self, path: &Path, volsize: u64) -> Result<SplitFile> {
        SplitFile::new(path, self, volsize)
    }
}

impl Volume {
    fn open(path: PathBuf, opts: &OpenOptions, first_open: &mut bool) -> Result<Volume> {
        Ok(Volume {
            file: Volume::open_file(path, opts, first_open)?,
            pos: 0,
            reset: false,
        })
    }

    fn open_file(path: PathBuf, opts: &OpenOptions, first_open: &mut bool) -> Result<File> {
        let w = match (*first_open, opts.append) {
            (false, true) => true,
            _ => opts.write,
        };
        let c = match (*first_open, opts.append, opts.create_new, opts.write) {
            (false, true, _, _) | (false, _, true, _) | (false, _, _, true) => true,
            _ => opts.create,
        };
        let cn = match (*first_open, opts.create_new) {
            (false, true) => false,
            _ => opts.create_new,
        };
        let t = match (*first_open, opts.truncate) {
            (false, true) => false,
            _ => opts.truncate,
        };

        if *first_open {
            *first_open = false;
        }

        OpenOptionsFs::new()
            .read(opts.read)
            .write(w)
            .append(false)
            .truncate(t)
            .create(c)
            .create_new(cn)
            .open(path)
    }

    fn init_volumes(path: &Path, opts: &OpenOptions, first_open: &mut bool) -> Result<Vec<Volume>> {
        Ok(Filenames::new(path.to_path_buf(), 1)
            .enumerate()
            .take_while(|(i, p)| *i == 0 || p.is_file())
            .map(|(_, p): (_, PathBuf)| -> Result<Volume> {
                Ok(Volume::open(p, opts, first_open)?)
            })
            .collect::<Result<Vec<Volume>>>()?)
    }

    fn truncate_volumes(path: &Path) -> Result<()> {
        for p in Filenames::new(path.to_path_buf(), 2) {
            if let Err(e) = fs::remove_file(p) {
                match e.kind() {
                    ErrorKind::NotFound => break,
                    _ => return Err(e),
                }
            }
        }
        Ok(())
    }

    fn chk_reset(&mut self) -> Result<()> {
        if self.reset {
            self.pos = self.file.seek(SeekFrom::Start(0))?;
            self.reset = false;
        }
        Ok(())
    }
}

impl Read for Volume {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let r = self.file.read(buf)?;
        self.pos += r as u64;
        Ok(r)
    }
}

impl Write for Volume {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let r = self.file.write(buf)?;
        self.pos += r as u64;
        Ok(r)
    }

    fn flush(&mut self) -> Result<()> {
        self.file.flush()
    }
}

impl SplitFile {
    /// Attempts to open a file in read-only mode.
    ///
    /// See the `OpenOptions::open` method for more details.
    pub fn open<P: AsRef<Path>>(path: P, volsize: u64) -> Result<SplitFile> {
        OpenOptions::new().read(true)._open(path.as_ref(), volsize)
    }

    /// Opens a file in write-only mode.
    ///
    /// This function will create a file if it does not exist,
    /// and will truncate it if it does.
    pub fn create<P: AsRef<Path>>(path: P, volsize: u64) -> Result<SplitFile> {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            ._open(path.as_ref(), volsize)
    }

    fn new(path: &Path, opts: &OpenOptions, volsize: u64) -> Result<SplitFile> {
        if opts.truncate {
            Volume::truncate_volumes(path)?;
        }

        let mut first_open = true;
        let vols = Volume::init_volumes(path, opts, &mut first_open)?;

        let mut sf = SplitFile {
            volumes: vols,
            opts: opts.clone(),
            path: path.to_path_buf(),
            volsize: volsize,
            index: 1,
            first_open: first_open,
        };

        if opts.append {
            sf.seek(SeekFrom::End(0))?;
        }
        Ok(sf)
    }

    fn add_volume(&mut self) -> Result<&mut Volume> {
        let index = self.volumes.len() + 1;
        self.volumes.push(Volume::open(
            Filenames::by_index(self.path.clone(), index),
            &self.opts,
            &mut self.first_open,
        )?);
        Ok(self.volumes.last_mut().unwrap())
    }

    fn len(&mut self) -> Result<u64> {
        let v: &mut Volume = self.volumes.last_mut().expect("No volumes exist");
        v.pos = v.file.seek(SeekFrom::End(0))?;
        v.reset = true;
        let last_file_size = v.pos;
        Ok((cmp::max(self.volumes.len() - 1, 0) as u64 * self.volsize) + last_file_size)
    }
}

impl Read for SplitFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let blen = buf.len();
        let mut rt: usize = 0;

        for (i, v) in self.volumes.iter_mut().enumerate().skip(self.index - 1) {
            self.index = i + 1;
            v.chk_reset()?;
            rt += v.read(&mut buf[rt..])?;
            if rt == blen {
                break;
            }
        }

        Ok(rt)
    }
}

impl Write for SplitFile {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let blen = buf.len();
        let volsize = self.volsize;
        let mut wt: usize = 0;

        for (i, v) in self.volumes.iter_mut().enumerate().skip(self.index - 1) {
            self.index = i + 1;
            v.chk_reset()?;
            let wlen = cmp::min(blen - wt, (volsize - v.pos) as usize);
            wt += v.write(&buf[wt..wt + wlen])?;
            if wt == blen {
                break;
            }
        }

        if wt < blen {
            for i in self.index.. {
                self.index = i + 1;
                let v: &mut Volume = self.add_volume()?;
                let wlen = cmp::min(blen - wt, (volsize - v.pos) as usize);
                wt += v.write(&buf[wt..wt + wlen])?;
                if wt == blen {
                    break;
                }
            }
        }

        Ok(wt)
    }

    fn flush(&mut self) -> Result<()> {
        for v in self.volumes.iter_mut() {
            v.flush()?;
        }
        Ok(())
    }
}

impl Seek for SplitFile {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        let mut filesize: u64 = 0;

        if let SeekFrom::End(_) = pos {
            filesize = self.len()?;
        }

        let mut apos: u64 = match pos {
            SeekFrom::Start(off) => off,
            SeekFrom::End(off) => safe_add(filesize, off)?,
            SeekFrom::Current(off) => safe_add(
                ((self.index - 1) as u64 * self.volsize) + self.volumes[self.index - 1].pos,
                off,
            )?,
        };

        //if absolute seek position is in or beyond last volume, prevent seek
        //beyond end of file
        if ((apos / self.volsize) + 1) as usize >= self.volumes.len() {
            if filesize == 0 {
                filesize = self.len()?;
            }
            apos = cmp::min(apos, filesize);
        }

        self.index = ((apos / self.volsize) + 1) as usize;
        let vpos = apos - ((self.index - 1) as u64 * self.volsize);

        self.volumes[self.index - 1].pos = self.volumes[self.index - 1]
            .file
            .seek(SeekFrom::Start(vpos))?;
        self.volumes[self.index - 1].reset = false;

        for v in self.volumes.iter_mut().skip(self.index) {
            v.reset = true;
        }

        Ok(apos)
    }
}

fn safe_add(nu64: u64, ni64: i64) -> Result<u64> {
    if ni64 >= 0 {
        Ok(nu64 + (ni64 as u64))
    } else {
        let ni64_flip = (-1 * ni64) as u64;
        if ni64_flip <= nu64 {
            Ok(nu64 - ni64_flip)
        } else {
            Err(Error::new(
                ErrorKind::InvalidInput,
                "Invalid Argument.  Cannot seek to negative position in the file.",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test() {
        let dir = tempdir().expect("tempdir error");
        let path = dir.path().join("test");
        let mut data: [u8; 99] = [0; 99];
        let mut rdata: [u8; 99] = [0; 99];

        for i in 0..99 {
            data[i] = i as u8;
        }

        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path.as_path(), 15)
            .expect("error opening file - create");
        file.write(&data).expect("write error");
        file.flush().expect("flush");
        drop(file);

        let mut file = SplitFile::open(path.as_path(), 15).expect("error opening file - read");
        file.read(&mut rdata).expect("read error");
        drop(file);

        for i in 0..data.len() {
            assert_eq!(rdata[i], data[i]);
        }

        let data2: [u8; 30] = [1; 30];
        let mut rdata2: [u8; 30] = [0; 30];

        let mut file = OpenOptions::new()
            .append(true)
            .read(true)
            .open(path.as_path(), 15)
            .expect("error opening file - append");

        file.write(&data2).expect("write error");
        file.flush().expect("flush");

        file.seek(SeekFrom::Current(-30)).expect("seek error");
        file.read(&mut rdata2).expect("read error");
        drop(file);

        assert_eq!(rdata2, data2);

        let mut rdata2: [u8; 30] = [0; 30];

        let mut file = OpenOptions::new()
            .truncate(true)
            .write(true)
            .open(path.as_path(), 15)
            .expect("file open error - truncate");

        file.write(&data2).expect("write error");
        file.flush().expect("flush");
        drop(file);

        let mut file = OpenOptions::new()
            .read(true)
            .open(path.as_path(), 15)
            .expect("open error - read");

        file.read(&mut rdata2).expect("read error");
        drop(file);

        assert_eq!(rdata2, data2);

        dir.close().expect("tempdir close error");
    }
}
