//! Process management syscalls

use crate::{mm::{PageTable, PageTableEntry, PhysAddr, VirtAddr, VirtPageNum}, task::{change_program_brk, current_user_token, exit_current_and_run_next, suspend_current_and_run_next}};
use crate::mm::PhysPageNum;
use crate::task::{mmap, munmap};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    -1
}

/// TODO: Finish sys_trace to pass testcase
/// HINT: You might reimplement it with virtual memory management.
/// 引入虚存机制后，原来内核的 sys_get_time 和 sys_trace 函数实现就无效了。请你重写这两个系统调用的代码，恢复其正常功能。

// 此外，由于本章我们有了地址空间作为隔离机制，所以 sys_trace 需要考虑一些额外的情况：

// 在读取（trace_request 为 0）时，如果对应地址用户不可见或不可读，则返回值应为 -1（isize 格式的 -1，而非 u8）。

// 在写入（trace_request 为 1）时，如果对应地址用户不可见或不可写，则返回值应为 -1（isize 格式的 -1，而非 u8）。

pub fn sys_trace(trace_request: usize, id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    let vaddr:usize = id;
    let page_table : PageTable = PageTable::from_token(current_user_token());
    let vppn: VirtPageNum = VirtAddr::from(vaddr).floor().into();
    let entry: Option<PageTableEntry> = page_table.translate(vppn);
    if entry.is_none() || !entry.unwrap().is_valid() || !entry.unwrap().is_user() {
        return -1;
    }
    if trace_request == 0 && !entry.unwrap().readable() {
        return -1;
    }
    if trace_request == 1 && !entry.unwrap().writable() {
        return -1;
    }
    let ppn: PhysPageNum = entry.unwrap().ppn();
    let mut addr:usize = PhysAddr::from(ppn).into();
    addr += VirtAddr::from(vaddr).page_offset();
    match trace_request {
        0 => {
            let address = addr as *const u8; 
            unsafe {
                let ret = *address;
                return ret as isize;
            }
        }
        1 => {
            let address = addr as *mut u8;
            unsafe {
                *address = _data as u8; 
            }
            return 0;
        }
        2 => { panic!("note implement"); 
        //return -1; 
        }
        _ => {return -1;}
    }
}

// mmap 和 munmap 匿名映射¶
// mmap 在 Linux 中主要用于在内存中映射文件， 本次实验简化它的功能，仅用于申请内存。

// 请实现 mmap 和 munmap 系统调用，mmap 定义如下：

// fn sys_mmap(start: usize, len: usize, prot: usize) -> isize
// syscall ID：222

// 申请长度为 len 字节的物理内存（不要求实际物理内存位置，可以随便找一块），将其映射到 start 开始的虚存，内存页属性为 prot

// 参数：
// start 需要映射的虚存起始地址，要求按页对齐

// len 映射字节长度，可以为 0

// prot：第 0 位表示是否可读，第 1 位表示是否可写，第 2 位表示是否可执行。其他位无效且必须为 0

// 返回值：执行成功则返回 0，错误返回 -1

// 说明：
// 为了简单，目标虚存区间要求按页对齐，len 可直接按页向上取整，不考虑分配失败时的页回收。

// 可能的错误：
// start 没有按页大小对齐

// prot & !0x7 != 0 (prot 其余位必须为0)

// prot & 0x7 = 0 (这样的内存无意义)

// [start, start + len) 中存在已经被映射的页

// 物理内存不足
// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    //KERNEL_SPACE.exclusive_access().area
    // let mut inner = TASK_MANAGER.inner.exclusive_access();
    // KERNEL_SPACE.exclusive_access().insert_framed_areainsert_framed_areainsert_framed_area(
    //         start.into(),
    //         (start + len).into(),
    //         MapPermission::R | MapPermission::U | MapPermission::X | MapPermission::W
    //     );
    //     0
    mmap(start, len, prot)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    munmap(start, len)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
