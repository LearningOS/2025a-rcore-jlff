//! `Arc<Inode>` -> `OSInodeInner`: In order to open files concurrently
//! we need to wrap `Inode` into `Arc`,but `Mutex` in `Inode` prevents
//! file systems from being accessed simultaneously
//!
//! `UPSafeCell<OSInodeInner>` -> `OSInode`: for static `ROOT_INODE`,we
//! need to wrap `OSInodeInner` into `UPSafeCell`
use super::File;
use crate::fs::StatMode;
use crate::{drivers::BLOCK_DEVICE, fs::Stat};
use crate::mm::UserBuffer;
use crate::sync::UPSafeCell;
use alloc::sync::Arc;
use alloc::vec::Vec;
use bitflags::*;
use easy_fs::{EasyFileSystem, Inode};
use lazy_static::*;

/// inode in memory
/// A wrapper around a filesystem inode
/// to implement File trait atop
/// 在本章的第一小节我们介绍过，站在用户的角度看来，在一个进程中可以使用多种不同的标志来打开一个文件，
/// 这会影响到打开的这个文件可以用何种方式被访问。
/// 此外，在连续调用 sys_read/write 读写一个文件的时候，
/// 我们知道进程中也存在着一个文件读写的当前偏移量，它也随着文件读写的进行而被不断更新。
/// 这些用户视角中的文件系统抽象特征需要内核来实现，
/// 与进程有很大的关系，而 easy-fs 文件系统不必涉及这些与进程结合紧密的属性。
/// 因此，我们需要将 easy-fs 提供的 Inode 加上上述信息，进一步封装为 OS 中的索引节点 OSInode ：
pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
}
/// The OS inode inner in 'UPSafeCell'
pub struct OSInodeInner {
    offset: usize,
    inode: Arc<Inode>,
}

impl OSInode {
    /// create a new inode in memory
    pub fn new(readable: bool, writable: bool, inode: Arc<Inode>) -> Self {
        Self {
            readable,
            writable,
            inner: unsafe { UPSafeCell::new(OSInodeInner { offset: 0, inode }) },
        }
    }
    /// read all data from the inode
    pub fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.exclusive_access();
        let mut buffer: Vec<u8> = Vec::with_capacity(512);
        buffer.resize(512, 0);
        let mut v: Vec<u8> = Vec::new();
        loop {
            let len = inner.inode.read_at(inner.offset, &mut buffer);
            if len == 0 {
                break;
            }
            inner.offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v
    }
}

// 在上一小节我们介绍过，为了使用 easy-fs 提供的抽象和服务，我们需要进行一些初始化操作才能成功将 easy-fs 接入到我们的内核中。按照前面总结的步骤：
// 打开块设备。从本节前面可以看出，我们已经打开并可以访问装载有 easy-fs 文件系统镜像的块设备 BLOCK_DEVICE ；
// 从块设备 BLOCK_DEVICE 上打开文件系统；
// 从文件系统中获取根目录的 inode 。
// 2-3 步我们在这里完成：
// 为了使用 easy-fs 提供的抽象，内核需要进行一些初始化操作。我们需要从块设备 BLOCK_DEVICE 上打开文件系统，并从文件系统中获取根目录的 inode 。
lazy_static! {
    pub static ref ROOT_INODE: Arc<Inode> = {
        let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
        Arc::new(EasyFileSystem::root_inode(&efs))
    };
}

/// List all apps in the root directory
/// 这之后就可以使用根目录的 inode ROOT_INODE ，在内核中调用 easy-fs 的相关接口了。
/// 例如，在文件系统初始化完毕之后，调用 list_apps 函数来打印所有可用应用的文件名：
pub fn list_apps() {
    println!("/**** APPS ****");
    for app in ROOT_INODE.ls() {
        println!("{}", app);
    }
    println!("**************/");
}

bitflags! {
    ///  The flags argument to the open() system call is constructed by ORing together zero or more of the following values:
    pub struct OpenFlags: u32 {
        /// readyonly
        const RDONLY = 0;
        /// writeonly
        const WRONLY = 1 << 0;
        /// read and write
        const RDWR = 1 << 1;
        /// create new file
        const CREATE = 1 << 9;
        /// truncate file size to 0
        const TRUNC = 1 << 10;
    }
}

impl OpenFlags {
    /// Do not check validity for simplicity
    /// Return (readable, writable)
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() {
            (true, false)
        } else if self.contains(Self::WRONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
}

/// Open a file
/// 这里主要是实现了 OpenFlags 各标志位的语义。
/// 例如只有 flags 参数包含 CREATE 标志位才允许创建文件；
/// 而如果文件已经存在，则清空文件的内容。
/// 另外我们将从 OpenFlags 解析得到的读写相关权限传入 OSInode 的创建过程中。
pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();
    if flags.contains(OpenFlags::CREATE) {
        if let Some(inode) = ROOT_INODE.find(name) {
            // clear size
            inode.clear();
            Some(Arc::new(OSInode::new(readable, writable, inode)))
        } else {
            // create file
            ROOT_INODE
                .create(name)
                .map(|inode| Arc::new(OSInode::new(readable, writable, inode)))
        }
    } else {
        ROOT_INODE.find(name).map(|inode| {
            if flags.contains(OpenFlags::TRUNC) {
                inode.clear();
            }
            Arc::new(OSInode::new(readable, writable, inode))
        })
    }
}

/// link
pub fn link(old_name: &str, new_name:&str) -> isize{
    trace!("inode link");
    //ROOT_INODE.link(old_name, new_name)
    if let Some(inode) = ROOT_INODE.find(old_name) {
        let nlink:u32 = inode.nlink();
        assert!(nlink >= 1);
        inode.set_nlink(nlink + 1);
        ROOT_INODE.link(new_name, inode.inode_no());
        0
    } else {
        -1
    }
}

/// unlink
pub fn unlink(path: &str) -> isize{
    trace!("inode unlink");
    if let Some(inode) = ROOT_INODE.find(path) {
        let nlink:u32 = inode.nlink();
        assert!(nlink >= 1);
        inode.set_nlink(nlink - 1);
        if nlink == 1 {
            trace!("inode unlink zero, remove data");
            inode.clear();
            ROOT_INODE.rm(path);
        }
        0
    } else {
        -1
    }
}

/// OSInode 也是要一种要放到进程文件描述符表中，通过 sys_read/write 进行读写的文件，我们需要为它实现 File Trait ：
impl File for OSInode {
    fn readable(&self) -> bool {
        self.readable
    }
    fn writable(&self) -> bool {
        self.writable
    }
    fn read(&self, mut buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_read_size = 0usize;
        for slice in buf.buffers.iter_mut() {
            let read_size = inner.inode.read_at(inner.offset, *slice);
            if read_size == 0 {
                break;
            }
            inner.offset += read_size;
            total_read_size += read_size;
        }
        total_read_size
    }
    fn write(&self, buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_write_size = 0usize;
        for slice in buf.buffers.iter() {
            let write_size = inner.inode.write_at(inner.offset, *slice);
            assert_eq!(write_size, slice.len());
            inner.offset += write_size;
            total_write_size += write_size;
        }
        total_write_size
    }
    fn stat(&self) -> Stat {
        let inode = &self.inner.exclusive_access().inode;
    
        Stat{
            dev: 0,
            ino: 0,
            mode: StatMode::FILE,
            nlink:inode.nlink(),
            pad: [0u64; 7],
        }
    }
}
