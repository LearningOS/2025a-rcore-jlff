//! Process management syscalls
use alloc::sync::Arc;

use crate::{
    config::PAGE_SIZE, loader::get_app_data_by_name, mm::{MapPermission, translated_refmut, translated_str}, task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next,
    }
};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}
// 在调用 sys_fork 之前，我们已经将当前进程 Trap 上下文中的 sepc 向后移动了 4 字节，
// 使得它回到用户态之后会从 ecall 的下一条指令开始执行。
// 之后，当我们复制地址空间时，子进程地址空间 Trap 上下文的 sepc 也是移动之后的值，我们无需再进行修改。
// 父子进程回到用户态的瞬间都处于刚刚从一次系统调用返回的状态，但二者返回值不同。
// 第 8~11 行我们将子进程的 Trap 上下文中用来存放系统调用返回值的 a0 寄存器修改为 0 ，
// 而父进程系统调用的返回值会在 syscall 返回之后再设置为 sys_fork 的返回值。
// 这就做到了父进程 fork 的返回值为子进程的 PID ，而子进程的返回值为 0。
pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
/// sys_waitpid 是一个立即返回的系统调用，它的返回值语义是：如果当前的进程不存在一个符合要求的子进程，则返回 -1；
/// 如果至少存在一个，但是其中没有僵尸进程（也即仍未退出）则返回 -2；
/// 如果都不是的话则可以正常回收并返回回收子进程的 pid 。
/// 但在编写应用的开发者看来， wait/waitpid 两个辅助函数都必定能够返回一个有意义的结果，要么是 -1，要么是一个正数 PID ，是不存在 -2 这种通过等待即可消除的中间结果的。
/// 等待的过程由用户库 user_lib 完成。
/// 首先判断 sys_waitpid 是否会返回 -1 ，这取决于当前进程是否有一个符合要求的子进程。
/// 当传入的 pid 为 -1 的时候，任何一个子进程都算是符合要求；但 pid 不为 -1 的时候，则只有 PID 恰好与 pid 相同的子进程才算符合条件。
/// 我们简单通过迭代器即可完成判断。
/// 再判断符合要求的子进程中是否有僵尸进程。如果找不到的话直接返回 -2 ，否则进行下一步处理：
/// 我们将子进程从向量中移除并置于当前上下文中，当它所在的代码块结束，
/// 这次引用变量的生命周期结束，子进程进程控制块的引用计数将变为 0 ，
/// 内核将彻底回收掉它占用的所有资源，包括内核栈、它的 PID 、存放页表的那些物理页帧等等。
/// 获得子进程退出码后，考虑到应用传入的指针指向应用地址空间，我们还需要手动查页表找到对应物理内存中的位置。
/// translated_refmut 的实现可以在 os/src/mm/page_table.rs 中找到。
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!("kernel::pid[{}] sys_waitpid [{}]", current_task().unwrap().pid.0, pid);
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_get_time",
        current_task().unwrap().pid.0
    );
    let us = get_time_us();
    // let page_table : PageTable = PageTable::from_token(current_user_token());
    // let addr:usize = page_table.translate_va(_ts).unwrap().into();

    // unsafe {
    //     let address: *mut TimeVal = addr as *mut TimeVal;
    //     *address = TimeVal {
    //         sec: us / 1_000_000,
    //         usec: us % 1_000_000,
    //     };
    // }
    *translated_refmut(current_user_token(), ts) = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
    };
    0
}

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mmap",
        current_task().unwrap().pid.0
    );
    if start % PAGE_SIZE != 0 {
            return -1
    }
    if prot & !07 != 0 || prot & 07 == 0 {
        return -1
    }
    let mut map_perm = MapPermission::U;
    if prot & 0x1 != 0 {
        map_perm |= MapPermission::R;
    }
    if prot & 0x2 != 0 {
        map_perm |= MapPermission::W;
    }
    if prot & 0x4 != 0 {
        map_perm |= MapPermission::X;
    }
    if current_task().unwrap().inner_exclusive_access().memory_set.is_mapped(start, len) {
        return -1;
    }
    current_task().unwrap().inner_exclusive_access().memory_set.insert_framed_area(start.into(), (start+len).into(), map_perm);
    0
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_munmap",
        current_task().unwrap().pid.0
    );
    if start % PAGE_SIZE != 0 {
        return -1
    } 

    if ! current_task().unwrap().inner_exclusive_access().memory_set.can_munmap(start, len) {
        return -1
    }
    current_task().unwrap().inner_exclusive_access().memory_set.remove_area_with_start_vpn(start.into());
    0
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
/// spawn 系统调用定义( 标准spawn看这里 )：

// fn sys_spawn(path: *const u8) -> isize
// syscall ID: 400

// 功能：新建子进程，使其执行目标程序。

// 说明：成功返回子进程id，否则返回 -1。

// 可能的错误：
// 无效的文件名。
// 虽然测例很简单，但提醒读者 spawn 不必 像 fork 一样复制父进程的地址空间。
pub fn sys_spawn(path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let current_task = current_task().unwrap();
        let new_task = current_task.spawn(data);
        let new_pid = new_task.pid.0;
        add_task(new_task);
        trace!(
            "kernel:new pid[{}] sys_spawn",
            new_pid
        );
        return new_pid as isize
    } else {
        return -1
    }
    //let current_task = current_task().unwrap();
    //let new_task = current_task.spawn();
    //let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    // let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // // we do not have to move to next instruction since we have done it before
    // // for child process, fork returns 0
    // trap_cx.x[10] = 0;
    // // add new task to scheduler
    // add_task(new_task);
    // new_pid as isize
    // // let child: isize = ;
    // // if child == 0 {
    // //     sys_exec(_path);
    // // }
    // // // else  
    // // //     return child
    // // -1
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    -1
}
