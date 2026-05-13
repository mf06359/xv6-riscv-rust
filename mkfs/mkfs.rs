use std::env;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::process;

const ROOTINO: u32 = 1;
const BSIZE: usize = 1024;
const FSMAGIC: u32 = 0x1020_3040;
const NDIRECT: usize = 12;
const NINDIRECT: usize = BSIZE / size_of::<u32>();
const MAXFILE: u32 = (NDIRECT + NINDIRECT) as u32;
const DIRSIZ: usize = 14;
const LOGBLOCKS: u32 = 30;
const FSSIZE: u32 = 2000;
const NINODES: u32 = 200;
const T_DIR: u16 = 1;
const T_FILE: u16 = 2;

const IPB: u32 = (BSIZE / size_of::<Dinode>()) as u32;
const BPB: u32 = (BSIZE * 8) as u32;

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct Superblock {
    magic: u32,
    size: u32,
    nblocks: u32,
    ninodes: u32,
    nlog: u32,
    logstart: u32,
    inodestart: u32,
    bmapstart: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct Dinode {
    inode_type: u16,
    major: u16,
    minor: u16,
    nlink: u16,
    size: u32,
    addrs: [u32; NDIRECT + 1],
}

impl Default for Dinode {
    fn default() -> Self {
        Self {
            inode_type: 0,
            major: 0,
            minor: 0,
            nlink: 0,
            size: 0,
            addrs: [0; NDIRECT + 1],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
struct Dirent {
    inum: u16,
    name: [u8; DIRSIZ],
}

impl Default for Dirent {
    fn default() -> Self {
        Self {
            inum: 0,
            name: [0; DIRSIZ],
        }
    }
}

#[inline(always)]
fn xshort(x: u16) -> u16 {
    x.to_le()
}

#[inline(always)]
fn xint(x: u32) -> u32 {
    x.to_le()
}

#[inline(always)]
fn from_le_u32(x: u32) -> u32 {
    u32::from_le(x)
}

#[inline(always)]
fn iblock(i: u32, sb: &Superblock) -> u32 {
    i / IPB + from_le_u32(sb.inodestart)
}

struct Mkfs {
    fsfd: File,
    sb: Superblock,
    nbitmap: u32,
    ninodeblocks: u32,
    nlog: u32,
    nmeta: u32,
    nblocks: u32,
    freeinode: u32,
    freeblock: u32,
}

impl Mkfs {
    fn new(path: &str) -> std::io::Result<Self> {
        let fsfd = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(path)?;

        let nbitmap = FSSIZE / BPB + 1;
        let ninodeblocks = NINODES / IPB + 1;
        let nlog = LOGBLOCKS + 1;
        let nmeta = 2 + nlog + ninodeblocks + nbitmap;
        let nblocks = FSSIZE - nmeta;

        let sb = Superblock {
            magic: xint(FSMAGIC),
            size: xint(FSSIZE),
            nblocks: xint(nblocks),
            ninodes: xint(NINODES),
            nlog: xint(nlog),
            logstart: xint(2),
            inodestart: xint(2 + nlog),
            bmapstart: xint(2 + nlog + ninodeblocks),
        };

        Ok(Self {
            fsfd,
            sb,
            nbitmap,
            ninodeblocks,
            nlog,
            nmeta,
            nblocks,
            freeinode: 1,
            freeblock: nmeta,
        })
    }

    fn wsect(&mut self, sec: u32, buf: &[u8; BSIZE]) -> std::io::Result<()> {
        self.fsfd
            .seek(SeekFrom::Start((sec as u64) * (BSIZE as u64)))?;
        self.fsfd.write_all(buf)?;
        Ok(())
    }

    fn rsect(&mut self, sec: u32, buf: &mut [u8; BSIZE]) -> std::io::Result<()> {
        self.fsfd
            .seek(SeekFrom::Start((sec as u64) * (BSIZE as u64)))?;
        self.fsfd.read_exact(buf)?;
        Ok(())
    }

    fn winode(&mut self, inum: u32, ip: &Dinode) -> std::io::Result<()> {
        let mut buf = [0u8; BSIZE];
        let bn = iblock(inum, &self.sb);
        self.rsect(bn, &mut buf)?;

        let dip = (inum % IPB) as usize;
        let off = dip * size_of::<Dinode>();
        let raw_ip = unsafe { std::slice::from_raw_parts((ip as *const Dinode).cast::<u8>(), size_of::<Dinode>()) };
        buf[off..off + size_of::<Dinode>()].copy_from_slice(raw_ip);
        self.wsect(bn, &buf)
    }

    fn rinode(&mut self, inum: u32, ip: &mut Dinode) -> std::io::Result<()> {
        let mut buf = [0u8; BSIZE];
        let bn = iblock(inum, &self.sb);
        self.rsect(bn, &mut buf)?;

        let dip = (inum % IPB) as usize;
        let off = dip * size_of::<Dinode>();
        let src = &buf[off..off + size_of::<Dinode>()];
        let dst = unsafe { std::slice::from_raw_parts_mut((ip as *mut Dinode).cast::<u8>(), size_of::<Dinode>()) };
        dst.copy_from_slice(src);
        Ok(())
    }

    fn ialloc(&mut self, inode_type: u16) -> std::io::Result<u32> {
        let inum = self.freeinode;
        self.freeinode += 1;

        let din = Dinode {
            inode_type: xshort(inode_type),
            major: 0,
            minor: 0,
            nlink: xshort(1),
            size: xint(0),
            addrs: [0; NDIRECT + 1],
        };
        self.winode(inum, &din)?;
        Ok(inum)
    }

    fn balloc(&mut self, used: u32) -> std::io::Result<()> {
        println!("balloc: first {} blocks have been allocated", used);
        assert!(used < BPB);

        let mut buf = [0u8; BSIZE];
        let mut i = 0;
        while i < used {
            let idx = (i / 8) as usize;
            buf[idx] |= 1 << (i % 8);
            i += 1;
        }
        println!(
            "balloc: write bitmap block at sector {}",
            from_le_u32(self.sb.bmapstart)
        );
        self.wsect(from_le_u32(self.sb.bmapstart), &buf)
    }

    fn iappend(&mut self, inum: u32, mut data: &[u8]) -> std::io::Result<()> {
        let mut din = Dinode::default();
        self.rinode(inum, &mut din)?;

        let mut off = from_le_u32(din.size);
        let mut buf = [0u8; BSIZE];

        while !data.is_empty() {
            let fbn = off / (BSIZE as u32);
            assert!(fbn < MAXFILE);

            let x: u32;
            if (fbn as usize) < NDIRECT {
                if from_le_u32(din.addrs[fbn as usize]) == 0 {
                    din.addrs[fbn as usize] = xint(self.freeblock);
                    self.freeblock += 1;
                }
                x = from_le_u32(din.addrs[fbn as usize]);
            } else {
                if from_le_u32(din.addrs[NDIRECT]) == 0 {
                    din.addrs[NDIRECT] = xint(self.freeblock);
                    self.freeblock += 1;
                }

                let indirect_blockno = from_le_u32(din.addrs[NDIRECT]);
                let mut indirect = [0u8; BSIZE];
                self.rsect(indirect_blockno, &mut indirect)?;

                let idx = (fbn as usize) - NDIRECT;
                let entry_off = idx * size_of::<u32>();
                let current = u32::from_le_bytes([
                    indirect[entry_off],
                    indirect[entry_off + 1],
                    indirect[entry_off + 2],
                    indirect[entry_off + 3],
                ]);

                let entry = if current == 0 {
                    let new_block = self.freeblock;
                    self.freeblock += 1;
                    let bytes = xint(new_block).to_le_bytes();
                    indirect[entry_off] = bytes[0];
                    indirect[entry_off + 1] = bytes[1];
                    indirect[entry_off + 2] = bytes[2];
                    indirect[entry_off + 3] = bytes[3];
                    self.wsect(indirect_blockno, &indirect)?;
                    new_block
                } else {
                    from_le_u32(current)
                };

                x = entry;
            }

            let n1 = usize::min(
                data.len(),
                (((fbn + 1) * (BSIZE as u32)).wrapping_sub(off)) as usize,
            );
            self.rsect(x, &mut buf)?;
            let start = (off - fbn * (BSIZE as u32)) as usize;
            buf[start..start + n1].copy_from_slice(&data[..n1]);
            self.wsect(x, &buf)?;

            data = &data[n1..];
            off += n1 as u32;
        }

        din.size = xint(off);
        self.winode(inum, &din)
    }
}

fn shortname_for_image(path: &str) -> &str {
    let s = path.strip_prefix("user/").unwrap_or(path);
    assert!(!s.contains('/'));
    s.strip_prefix('_').unwrap_or(s)
}

fn dirent_as_bytes(de: &Dirent) -> &[u8] {
    unsafe { std::slice::from_raw_parts((de as *const Dirent).cast::<u8>(), size_of::<Dirent>()) }
}

fn die(msg: &str) -> ! {
    eprintln!("{}", msg);
    process::exit(1);
}

fn run() -> std::io::Result<()> {
    assert_eq!(size_of::<i32>(), 4);
    assert_eq!(BSIZE % size_of::<Dinode>(), 0);
    assert_eq!(BSIZE % size_of::<Dirent>(), 0);

    let argv: Vec<String> = env::args().collect();
    if argv.len() < 2 {
        die("Usage: mkfs fs.img files...");
    }

    let mut mkfs = Mkfs::new(&argv[1])?;
    println!(
        "nmeta {} (boot, super, log blocks {}, inode blocks {}, bitmap blocks {}) blocks {} total {}",
        mkfs.nmeta, mkfs.nlog, mkfs.ninodeblocks, mkfs.nbitmap, mkfs.nblocks, FSSIZE
    );

    let zeroes = [0u8; BSIZE];
    let mut i = 0;
    while i < FSSIZE {
        mkfs.wsect(i, &zeroes)?;
        i += 1;
    }

    let mut sb_block = [0u8; BSIZE];
    let raw_sb = unsafe {
        std::slice::from_raw_parts(
            (&mkfs.sb as *const Superblock).cast::<u8>(),
            size_of::<Superblock>(),
        )
    };
    sb_block[..size_of::<Superblock>()].copy_from_slice(raw_sb);
    mkfs.wsect(1, &sb_block)?;

    let rootino = mkfs.ialloc(T_DIR)?;
    assert_eq!(rootino, ROOTINO);

    let mut de = Dirent::default();
    de.inum = xshort(rootino as u16);
    de.name[0] = b'.';
    mkfs.iappend(rootino, dirent_as_bytes(&de))?;

    de = Dirent::default();
    de.inum = xshort(rootino as u16);
    de.name[0] = b'.';
    de.name[1] = b'.';
    mkfs.iappend(rootino, dirent_as_bytes(&de))?;

    for src_path in argv.iter().skip(2) {
        let short = shortname_for_image(src_path);
        assert!(short.len() <= DIRSIZ);

        let mut fd = File::open(src_path)?;
        let inum = mkfs.ialloc(T_FILE)?;

        let mut entry = Dirent::default();
        entry.inum = xshort(inum as u16);
        entry.name[..short.len()].copy_from_slice(short.as_bytes());
        mkfs.iappend(rootino, dirent_as_bytes(&entry))?;

        let mut buf = [0u8; BSIZE];
        loop {
            let cc = fd.read(&mut buf)?;
            if cc == 0 {
                break;
            }
            mkfs.iappend(inum, &buf[..cc])?;
        }
    }

    let mut din = Dinode::default();
    mkfs.rinode(rootino, &mut din)?;
    let mut off = from_le_u32(din.size);
    off = ((off / (BSIZE as u32)) + 1) * (BSIZE as u32);
    din.size = xint(off);
    mkfs.winode(rootino, &din)?;

    mkfs.balloc(mkfs.freeblock)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{}", e);
        process::exit(1);
    }
}
