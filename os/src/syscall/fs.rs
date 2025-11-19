//! File and filesystem-related syscalls


use crate::fs::{OpenFlags, Stat, link, unlink, open_file};
use crate::mm::{UserBuffer, translated_byte_buffer, translated_refmut, translated_str};
use crate::task::{current_task, current_user_token};

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// YOUR JOB: Implement fstat.
/// 参数：
// fd: 文件描述符
// st: 文件状态结构体

// #[repr(C)]
// #[derive(Debug)]
// pub struct Stat {
//     /// 文件所在磁盘驱动器号，该实验中写死为 0 即可
//     pub dev: u64,
//     /// inode 文件所在 inode 编号
//     pub ino: u64,
//     /// 文件类型
//     pub mode: StatMode,
//     /// 硬链接数量，初始为1
//     pub nlink: u32,
//     /// 无需考虑，为了兼容性设计
//     pad: [u64; 7],
// }

// /// StatMode 定义：
// bitflags! {
//     pub struct StatMode: u32 {
//         const NULL  = 0;
//         /// directory
//         const DIR   = 0o040000;
//         /// ordinary regular file
//         const FILE  = 0o100000;
//     }
// }
pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    trace!(
        "kernel:pid[{}] sys_fstat",
        current_task().unwrap().pid.0
    );
    let task = current_task().unwrap();
    let token = current_user_token();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        *translated_refmut(token, st) = file.stat();
        0
    } else {
        -1  // File descriptor not found
    }
}

/// YOUR JOB: Implement linkat.
/// syscall ID: 37

// 功能：创建一个文件的一个硬链接， linkat标准接口 。

// Ｃ接口： int linkat(int olddirfd, char* oldpath, int newdirfd, char* newpath, unsigned int flags)

// Rust 接口： fn linkat(olddirfd: i32, oldpath: *const u8, newdirfd: i32, newpath: *const u8, flags: u32) -> i32

// 参数：
// olddirfd，newdirfd: 仅为了兼容性考虑，本次实验中始终为 AT_FDCWD (-100)，可以忽略。

// flags: 仅为了兼容性考虑，本次实验中始终为 0，可以忽略。

// oldpath：原有文件路径

// newpath: 新的链接文件路径。

// 说明：
// 为了方便，不考虑新文件路径已经存在的情况（属于未定义行为）。除非出现新旧名字一致的情况，此时需要返回-1。

// 返回值：如果出现了错误则返回 -1，否则返回 0。

// 可能的错误
// 链接同名文件
pub fn sys_linkat(old_name: *const u8, new_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_linkat",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let old_path = translated_str(token, old_name);
    let new_path = translated_str(token, new_name);
    if old_path == new_path {
        return -1;
    }
    assert!(old_path != new_path);
    link(old_path.as_str(), new_path.as_str())
}

/// YOUR JOB: Implement unlinkat.
/// syscall ID: 35

// 功能：取消一个文件路径到文件的链接, unlinkat标准接口 。

// Ｃ接口： int unlinkat(int dirfd, char* path, unsigned int flags)

// Rust 接口： fn unlinkat(dirfd: i32, path: *const u8, flags: u32) -> i32

// 参数：
// dirfd: 仅为了兼容性考虑，本次实验中始终为 AT_FDCWD (-100)，可以忽略。

// flags: 仅为了兼容性考虑，本次实验中始终为 0，可以忽略。

// path：文件路径。

// 说明：
// 注意考虑使用 unlink 彻底删除文件的情况，此时需要回收inode以及它对应的数据块。

// 返回值：如果出现了错误则返回 -1，否则返回 0。

// 可能的错误
// 文件不存在。
pub fn sys_unlinkat(name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_unlinkat",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, name);
    unlink(path.as_str())
}
